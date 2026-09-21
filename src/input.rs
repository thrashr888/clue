//! Source-independent record ingestion and explicit argv execution.
use crate::{Candidate, api};
use anyhow::{Context, Result, ensure};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

const MAX_BYTES: usize = 4_194_304;
pub const PROFILES: &[(&str, &str)] = &[
    (
        "github-issues",
        include_str!("../profiles/github-issues.json"),
    ),
    ("beads-ready", include_str!("../profiles/beads-ready.json")),
    (
        "cider-reminders",
        include_str!("../profiles/cider-reminders.json"),
    ),
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub id: Option<String>,
    pub title: Option<String>,
    pub text: Option<String>,
    pub reference: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub argv: Vec<String>,
    pub mapping: Mapping,
}

pub fn profile(name: &str) -> Result<Profile> {
    let (_, body) = PROFILES
        .iter()
        .find(|(n, _)| *n == name)
        .context("unknown profile; run clue profiles list")?;
    Ok(serde_json::from_str(body)?)
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum Format {
    #[default]
    Auto,
    Json,
    Jsonl,
}

#[derive(Debug, Args)]
pub struct InputArgs {
    /// Read stdin (-) or a file. Mutually exclusive with command execution.
    #[arg(long, default_value = "-")]
    pub input: String,
    #[arg(long, value_enum, default_value = "auto")]
    pub format: Format,
    /// Built-in field mapping and optional command recipe.
    #[arg(long, conflicts_with = "profile_file")]
    pub profile: Option<String>,
    /// Explicit JSON profile file; never auto-loaded from a repository.
    #[arg(long)]
    pub profile_file: Option<PathBuf>,
    /// Execute the chosen profile's argv instead of reading stdin.
    #[arg(long, conflicts_with = "command")]
    pub run_profile: bool,
    /// Field name or JSON Pointer (such as /issue/number).
    #[arg(long)]
    pub id: Option<String>,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long = "ref")]
    pub reference: Option<String>,
    #[arg(long)]
    pub source: Option<String>,
    /// Execute these literal argv after --; no shell expansion or model-generated commands.
    #[arg(last = true, num_args = 1..)]
    pub command: Vec<String>,
}

pub struct Collected {
    pub items: Vec<Candidate>,
    pub provenance: Value,
    pub upstream: Option<Value>,
}
impl Collected {
    pub fn envelope(&self) -> Value {
        json!({"schema_version":1,"ok":true,"mode":"collected","partial":self.upstream.as_ref().is_some_and(|u| u["partial"] == true || u["truncated"] == true),"input":self.provenance,"upstream":self.upstream,"results":self.items})
    }
}

pub fn parse(bytes: &[u8], format: Format) -> Result<(Vec<Value>, Option<Value>)> {
    ensure!(bytes.len() <= MAX_BYTES, "input exceeds 4 MiB");
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok((Vec::new(), None));
    }
    let single = match format {
        Format::Jsonl => None,
        _ => serde_json::from_slice::<Value>(bytes).ok(),
    };
    if let Some(mut value) = single {
        if let Some(items) = value.as_array_mut() {
            return Ok((std::mem::take(items), None));
        }
        ensure!(
            value.is_object(),
            "input must be records, a JSON array, or a results envelope"
        );
        if value.get("results").is_some() || value.get("schema_version").is_some() {
            ensure!(value["ok"] != false, "upstream command reported failure");
            let items = value
                .get_mut("results")
                .and_then(Value::as_array_mut)
                .context("envelope requires a results array")?;
            let items = std::mem::take(items);
            value.as_object_mut().unwrap().remove("results");
            return Ok((items, Some(value)));
        }
        return Ok((vec![value], None));
    }
    ensure!(!matches!(format, Format::Json), "input is not valid JSON");
    let text = std::str::from_utf8(bytes).context("input must be UTF-8")?;
    let mut rows = Vec::new();
    for (index, line) in text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        let row: Value = serde_json::from_str(line)
            .with_context(|| format!("invalid JSONL record on line {}", index + 1))?;
        ensure!(row.is_object(), "JSONL records must be objects");
        rows.push(row);
    }
    Ok((rows, None))
}

fn field<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.starts_with('/') {
        value.pointer(path)
    } else {
        value.get(path)
    }
}

pub fn normalize(rows: Vec<Value>, mapping: &Mapping, mapped: bool) -> Result<Vec<Candidate>> {
    ensure!(
        rows.len() <= 50,
        "at most 50 records accepted; narrow retrieval in the source CLI"
    );
    let mut items = Vec::new();
    for row in rows {
        ensure!(row.is_object(), "each input record must be an object");
        if !mapped && let Ok(candidate) = serde_json::from_value::<Candidate>(row.clone()) {
            items.push(candidate);
            continue;
        }
        let id = field(&row, mapping.id.as_deref().unwrap_or("id"))
            .context("record has no mapped ID")?;
        let id = match id {
            Value::String(v) => v.clone(),
            Value::Number(v) if v.is_i64() || v.is_u64() => v.to_string(),
            _ => anyhow::bail!("record ID must be text or an integer"),
        };
        let title = field(&row, mapping.title.as_deref().unwrap_or("title"))
            .and_then(Value::as_str)
            .context("record title must be text")?
            .to_owned();
        let text = match field(&row, mapping.text.as_deref().unwrap_or("text")) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(v)) => v.clone(),
            _ => anyhow::bail!("mapped text must be text or null"),
        };
        let location = match mapping.reference.as_deref().and_then(|p| field(&row, p)) {
            None | Some(Value::Null) => None,
            Some(Value::String(v)) => Some(v.clone()),
            _ => anyhow::bail!("mapped reference must be text or null"),
        };
        items.push(Candidate {
            id,
            title,
            text,
            source: mapping.source.clone().unwrap_or_else(|| "input".into()),
            location,
            modified: None,
            event: None,
            lexical_score: 0.0,
            relevance: None,
            record: Some(row),
        });
    }
    if !items.is_empty() {
        api::validate_candidates(&items)?;
    }
    Ok(items)
}

async fn bounded<R: AsyncRead + Unpin>(reader: R) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(bytes.len() <= MAX_BYTES, "command output exceeds 4 MiB");
    Ok(bytes)
}

pub async fn execute(argv: &[String], timeout: Duration) -> Result<Vec<u8>> {
    let executable = argv.first().context("command requires an executable")?;
    ensure!(!executable.is_empty(), "command executable cannot be empty");
    let mut child = Command::new(executable)
        .args(&argv[1..])
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("TYPESAFE_API_KEY_FILE")
        .env_remove("CLUE_PROVIDER_API_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("cannot launch requested command")?;
    let stdout = child.stdout.take().context("missing command stdout")?;
    let stderr = child.stderr.take().context("missing command stderr")?;
    let work = async {
        let (stdout, _, status) = tokio::try_join!(bounded(stdout), bounded(stderr), async {
            Ok::<_, anyhow::Error>(child.wait().await?)
        })?;
        ensure!(
            status.success(),
            "source command failed ({status}); run it directly for diagnostics"
        );
        Ok(stdout)
    };
    let result = tokio::time::timeout(timeout, work).await;
    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(error)) => {
            let _ = child.kill().await;
            Err(error)
        }
        Err(_) => {
            let _ = child.kill().await;
            anyhow::bail!("source command timed out")
        }
    }
}

pub async fn collect(args: &InputArgs, timeout: Duration) -> Result<Collected> {
    let profile = if let Some(name) = &args.profile {
        Some(profile(name)?)
    } else if let Some(path) = &args.profile_file {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .context("cannot open profile")?
            .take(65_537)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65_536, "profile exceeds 64 KiB");
        Some(serde_json::from_slice::<Profile>(&bytes).context("invalid JSON profile")?)
    } else {
        None
    };
    let mut mapping = profile
        .as_ref()
        .map(|p| p.mapping.clone())
        .unwrap_or_default();
    let mapped = profile.is_some()
        || args.id.is_some()
        || args.title.is_some()
        || args.text.is_some()
        || args.reference.is_some()
        || args.source.is_some();
    for (slot, value) in [
        (&mut mapping.id, &args.id),
        (&mut mapping.title, &args.title),
        (&mut mapping.text, &args.text),
        (&mut mapping.reference, &args.reference),
        (&mut mapping.source, &args.source),
    ] {
        if value.is_some() {
            *slot = value.clone();
        }
    }
    let argv = if args.run_profile {
        &profile
            .as_ref()
            .context("--run-profile requires --profile or --profile-file")?
            .argv
    } else {
        &args.command
    };
    ensure!(
        !args.run_profile || !argv.is_empty(),
        "profile has no command to run"
    );
    ensure!(
        argv.is_empty() || args.input == "-",
        "--input cannot be combined with command execution"
    );
    let (bytes, provenance) = if !argv.is_empty() {
        (
            execute(argv, timeout).await?,
            json!({"kind":"command","executable":argv[0],"profile":profile.as_ref().map(|p| &p.name)}),
        )
    } else {
        let reader: Box<dyn Read> = if args.input == "-" {
            Box::new(std::io::stdin())
        } else {
            Box::new(std::fs::File::open(&args.input).context("cannot open input file")?)
        };
        let mut bytes = Vec::new();
        reader
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        (
            bytes,
            json!({"kind":if args.input == "-" {"stdin"} else {"file"},"profile":profile.as_ref().map(|p| &p.name)}),
        )
    };
    let (rows, upstream) = parse(&bytes, args.format)?;
    Ok(Collected {
        items: normalize(rows, &mapping, mapped)?,
        provenance,
        upstream,
    })
}
