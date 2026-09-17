use crate::{Candidate, clip};
use anyhow::{Context, Result, ensure};
use clap::ValueEnum;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Reminders,
    Notes,
    Safari,
    Files,
}
impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Self::Reminders => "reminders",
            Self::Notes => "notes",
            Self::Safari => "safari",
            Self::Files => "files",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum Status {
    Open,
    Completed,
    All,
}

#[derive(Clone)]
pub struct SearchOptions {
    pub query: String,
    pub sources: Vec<Source>,
    pub directory: Option<PathBuf>,
    pub list: Option<String>,
    pub status: Status,
    pub note_bodies: bool,
    pub limit: usize,
    pub cider: PathBuf,
    pub timeout: Duration,
}

#[derive(Debug, Serialize)]
pub struct SourceReport {
    pub source: String,
    pub ok: bool,
    pub fetched: usize,
    pub matched: usize,
    pub elapsed_ms: u128,
    pub coverage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub query: String,
    pub mode: String,
    pub partial: bool,
    pub elapsed_ms: u128,
    pub sources: Vec<SourceReport>,
    pub results: Vec<Candidate>,
}

async fn read_bounded<R: AsyncRead + Unpin>(reader: R) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(8_388_609).read_to_end(&mut bytes).await?;
    ensure!(bytes.len() <= 8_388_608, "Cider output exceeded 8 MiB");
    Ok(bytes)
}

pub async fn run_cider(binary: &Path, args: &[String], timeout: Duration) -> Result<Value> {
    let mut child = Command::new(binary)
        .args(args)
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("TYPESAFE_API_KEY_FILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("cannot launch Cider; install it or set --cider")?;
    let stdout = child.stdout.take().context("Cider stdout unavailable")?;
    let stderr = child.stderr.take().context("Cider stderr unavailable")?;
    let work = async {
        let (out, err, status) =
            tokio::join!(read_bounded(stdout), read_bounded(stderr), child.wait());
        let out = out?;
        let _err = err?;
        let status = status?;
        ensure!(
            status.success(),
            "Cider command failed (status {}); check cider doctor and macOS permissions",
            status
        );
        let value: Value = serde_json::from_slice(&out).context("Cider returned invalid JSON")?;
        ensure!(
            value.get("ok") != Some(&Value::Bool(false)) && value.get("error").is_none(),
            "Cider reported a source error; check cider doctor"
        );
        Ok(value)
    };
    match tokio::time::timeout(timeout, work).await {
        Ok(result) => result,
        Err(_) => {
            let _ = child.kill().await;
            anyhow::bail!("Cider source timed out after {} seconds", timeout.as_secs())
        }
    }
}

pub fn terms(query: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "a", "an", "my", "me", "find", "show", "about", "for", "and", "or", "of", "in",
        "on", "to", "with", "please", "where", "is", "was", "that", "it", "i",
    ];
    let mut seen = HashSet::new();
    query
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|s| s.chars().count() > 1 && !STOP.contains(&s.as_str()) && seen.insert(s.clone()))
        .take(12)
        .collect()
}

pub fn lexical(query: &str, title: &str, text: &str) -> f64 {
    let title = title.to_lowercase();
    let text = text.to_lowercase();
    let tokens = terms(query);
    if tokens.is_empty() {
        return 0.0;
    }
    let score: f64 = tokens
        .iter()
        .map(|t| {
            if title.contains(t) {
                4.0
            } else if text.contains(t) {
                1.0
            } else {
                0.0
            }
        })
        .sum();
    score / tokens.len() as f64
        + if title.contains(&query.to_lowercase()) {
            2.0
        } else {
            0.0
        }
}

fn excerpt(text: &str, query: &str) -> String {
    let lower = text.to_lowercase();
    let position = terms(query)
        .iter()
        .filter_map(|term| lower.find(term))
        .min()
        .unwrap_or(0);
    // Lowercasing can change byte lengths. Convert approximately to a char position,
    // then operate only on valid Unicode scalar boundaries.
    let before = lower
        .char_indices()
        .take_while(|(i, _)| *i < position)
        .count();
    text.chars()
        .skip(before.saturating_sub(100))
        .take(1600)
        .collect()
}

fn string(row: &Value, key: &str) -> String {
    row[key].as_str().unwrap_or("").into()
}

fn file_text(path: &Path, root: &Path) -> String {
    let Ok(path) = path.canonicalize() else {
        return String::new();
    };
    if !path.starts_with(root)
        || path.components().any(|c| {
            matches!(
                c.as_os_str().to_str(),
                Some(".git" | "node_modules" | "target" | ".env")
            )
        })
    {
        return String::new();
    }
    if !matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("md" | "txt" | "rs" | "py" | "ts" | "js" | "toml" | "yaml" | "yml")
    ) {
        return String::new();
    }
    use std::io::Read;
    let Ok(file) = std::fs::File::open(&path) else {
        return String::new();
    };
    let mut bytes = Vec::new();
    if file.take(65536).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn command(source: Source, options: &SearchOptions) -> Result<Vec<String>> {
    let mut args: Vec<String> = match source {
        Source::Reminders => ["reminders", "list", "--limit", "500"]
            .map(String::from)
            .to_vec(),
        Source::Notes => [
            "notes",
            "list",
            "--limit",
            if options.note_bodies { "30" } else { "200" },
        ]
        .map(String::from)
        .to_vec(),
        Source::Safari => ["safari", "history", "--limit", "300"]
            .map(String::from)
            .to_vec(),
        Source::Files => {
            let root = options
                .directory
                .as_ref()
                .context("files source requires --directory")?
                .canonicalize()
                .context("cannot resolve search directory")?;
            ensure!(root.is_dir(), "--directory must be a directory");
            let query = terms(&options.query)
                .iter()
                .map(|t| format!("(kMDItemFSName == '*{t}*'cd || kMDItemTextContent == '*{t}*'cd)"))
                .collect::<Vec<_>>()
                .join(" || ");
            ensure!(!query.is_empty(), "query contains no searchable terms");
            vec![
                "spotlight".into(),
                "--query".into(),
                query,
                "--directory".into(),
                root.to_string_lossy().into_owned(),
            ]
        }
    };
    if source == Source::Reminders {
        if let Some(list) = &options.list {
            args.extend(["--list".into(), list.clone()]);
        }
        if options.status != Status::Open {
            args.push("--include-completed".into());
        }
    }
    if source == Source::Notes && !options.note_bodies {
        args.push("--brief".into());
    }
    Ok(args)
}

pub fn normalize(source: Source, value: &Value, options: &SearchOptions) -> Result<Vec<Candidate>> {
    let rows = value.as_array().context("expected a Cider JSON array")?;
    let root = options
        .directory
        .as_ref()
        .and_then(|p| p.canonicalize().ok());
    let mut result = Vec::new();
    for row in rows {
        ensure!(row.is_object(), "invalid Cider record");
        if source == Source::Reminders {
            let completed = row["completed"]
                .as_bool()
                .context("reminder has no completion state")?;
            if (options.status == Status::Open && completed)
                || (options.status == Status::Completed && !completed)
            {
                continue;
            }
            if options
                .list
                .as_ref()
                .is_some_and(|list| !string(row, "list").eq_ignore_ascii_case(list))
            {
                continue;
            }
        }
        let (title, content, location, id) = match source {
            Source::Reminders => (
                string(row, "title"),
                format!("{}\n{}", string(row, "list"), string(row, "notes")),
                None,
                string(row, "id"),
            ),
            Source::Notes => (
                string(row, "title"),
                format!("{}\n{}", string(row, "folder"), string(row, "body")),
                None,
                string(row, "id"),
            ),
            Source::Safari => (
                string(row, "title"),
                string(row, "url"),
                Some(string(row, "url")),
                string(row, "url"),
            ),
            Source::Files => {
                let path = PathBuf::from(string(row, "path"));
                let canonical = path.canonicalize().ok();
                if !canonical
                    .as_ref()
                    .zip(root.as_ref())
                    .is_some_and(|(p, r)| p.starts_with(r))
                {
                    continue;
                }
                (
                    string(row, "name"),
                    root.as_ref()
                        .map(|r| file_text(&path, r))
                        .unwrap_or_default(),
                    Some(path.to_string_lossy().into_owned()),
                    path.to_string_lossy().into_owned(),
                )
            }
        };
        ensure!(!id.is_empty(), "Cider record has no stable identity");
        let score = lexical(&options.query, &title, &content);
        if score <= 0.0 {
            continue;
        }
        let modified = ["modified_at", "modified", "last_visited"]
            .iter()
            .find_map(|k| row[k].as_str().map(String::from));
        result.push(Candidate {
            id: format!("{}:{id}", source.name()),
            source: source.name().into(),
            title: clip(&title, 500),
            text: excerpt(&content, &options.query),
            location,
            modified,
            lexical_score: score,
            relevance: None,
        });
    }
    Ok(result)
}

async fn fetch(source: Source, options: SearchOptions) -> (SourceReport, Vec<Candidate>) {
    let start = Instant::now();
    let result = async {
        let args = command(source, &options)?;
        let value = run_cider(&options.cider, &args, options.timeout).await?;
        let count = value.as_array().context("expected source array")?.len();
        Ok::<_, anyhow::Error>((count, normalize(source, &value, &options)?))
    }
    .await;
    let coverage=match source {Source::Reminders=>"Bounded read; Cider may cap at 500/store before list filtering. Order is not verified manual priority.",Source::Notes=>if options.note_bodies{"First 30 notes with bodies; not an exhaustive note search."}else{"Up to 200 note titles/folders; bodies were not read."},Source::Safari=>"Up to 300 recent Safari history entries; other browsers and bookmarks are not included.",Source::Files=>"Up to 200 Spotlight hits in the selected directory; text previews read at most 64 KiB/file."}.into();
    match result {
        Ok((fetched, items)) => (
            SourceReport {
                source: source.name().into(),
                ok: true,
                fetched,
                matched: items.len(),
                elapsed_ms: start.elapsed().as_millis(),
                coverage,
                error: None,
            },
            items,
        ),
        Err(error) => (
            SourceReport {
                source: source.name().into(),
                ok: false,
                fetched: 0,
                matched: 0,
                elapsed_ms: start.elapsed().as_millis(),
                coverage,
                error: Some(error.to_string()),
            },
            Vec::new(),
        ),
    }
}

pub async fn search(options: SearchOptions) -> Result<SearchResult> {
    crate::validate_query(&options.query)?;
    ensure!(
        !terms(&options.query).is_empty(),
        "query contains no searchable terms"
    );
    ensure!((1..=50).contains(&options.limit), "limit must be 1–50");
    ensure!(!options.sources.is_empty(), "select at least one source");
    if options.sources.contains(&Source::Files) {
        command(Source::Files, &options)?;
    }
    let start = Instant::now();
    let mut jobs = tokio::task::JoinSet::new();
    let mut seen = HashSet::new();
    for source in &options.sources {
        if seen.insert(source.name()) {
            jobs.spawn(fetch(*source, options.clone()));
        }
    }
    let mut reports = Vec::new();
    let mut candidates = Vec::new();
    while let Some(job) = jobs.join_next().await {
        let (report, items) = job.context("source worker failed")?;
        reports.push(report);
        candidates.extend(items);
    }
    reports.sort_by(|a, b| a.source.cmp(&b.source));
    candidates.sort_by(|a, b| {
        b.lexical_score
            .total_cmp(&a.lexical_score)
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut ids = HashSet::new();
    candidates.retain(|c| ids.insert(c.id.clone()));
    candidates.truncate(options.limit);
    Ok(SearchResult {
        query: options.query,
        mode: "local_lexical".into(),
        partial: reports.iter().any(|r| !r.ok),
        elapsed_ms: start.elapsed().as_millis(),
        sources: reports,
        results: candidates,
    })
}

pub fn bounded_context(items: &[Candidate], budget: usize) -> Result<(Vec<Candidate>, usize)> {
    ensure!(
        (256..=100_000).contains(&budget),
        "context budget must be 256–100000 bytes"
    );
    let mut output = Vec::new();
    for item in items {
        output.push(item.clone());
        if serde_json::to_vec(&output)?.len() > budget {
            output.pop();
        }
    }
    let bytes = serde_json::to_vec(&output)?.len();
    Ok((output, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn options() -> SearchOptions {
        SearchOptions {
            query: "sync".into(),
            sources: vec![Source::Reminders],
            directory: None,
            list: Some("Project".into()),
            status: Status::Completed,
            note_bodies: false,
            limit: 5,
            cider: "cider".into(),
            timeout: Duration::from_secs(1),
        }
    }
    #[test]
    fn filters_before_limit_and_preserves_unique_identity() {
        let rows = json!([{"id":"1","title":"sync","list":"Project","completed":false},{"id":"2","title":"sync","list":"Project","completed":true},{"id":"3","title":"sync","list":"Other","completed":true}]);
        let items = normalize(Source::Reminders, &rows, &options()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "reminders:2");
        assert!(
            normalize(
                Source::Reminders,
                &json!([{"id":"4","title":"sync"}]),
                &options()
            )
            .is_err()
        );
    }
    #[test]
    fn no_shell_or_arbitrary_metadata_query() {
        let temp = tempfile::tempdir().unwrap();
        let mut o = options();
        o.query = "foo' || rm -rf /".into();
        o.directory = Some(temp.path().into());
        let args = command(Source::Files, &o).unwrap();
        assert!(!args[2].contains("foo'"));
        assert_eq!(args[0], "spotlight");
        o.list = Some("$(touch /tmp/never)".into());
        let args = command(Source::Reminders, &o).unwrap();
        assert!(args.contains(&"$(touch /tmp/never)".into()));
    }
    #[test]
    fn byte_budget_and_unicode_are_honest() {
        let a: Candidate =
            serde_json::from_value(json!({"id":"a","title":"あ".repeat(200),"text":"hello"}))
                .unwrap();
        let b: Candidate = serde_json::from_value(json!({"id":"b","title":"short"})).unwrap();
        let (items, bytes) = bounded_context(&[a, b], 300).unwrap();
        assert!(bytes <= 300);
        assert_eq!(items[0].id, "b");
        assert_eq!(clip("あいう", 2), "あい");
    }
}
