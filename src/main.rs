use anyhow::{Context, Result, ensure};
use cider_ai::{
    Candidate, api, credentials,
    search::{self, Source, Status},
    skills,
};
use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(
    version,
    about = "Local Mac search and agent context; optional TypeSafe relevance ranking"
)]
struct Cli {
    #[arg(long, global = true)]
    pretty: bool,
    #[arg(long, global = true, env = "CIDER_BIN", default_value = "cider")]
    cider: PathBuf,
    #[arg(
        long,
        global = true,
        env = "TYPESAFE_MODEL",
        default_value = "jev-latest"
    )]
    model: String,
    #[arg(long,global=true,default_value_t=15,value_parser=clap::value_parser!(u64).range(1..=60))]
    timeout: u64,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Search local Cider sources without sending data unless --ai is selected.
    Search(SearchArgs),
    /// Build a bounded evidence bundle for an agent.
    Context {
        #[command(flatten)]
        search: SearchArgs,
        #[arg(long, default_value_t = 12000)]
        budget_bytes: usize,
    },
    /// Rank a JSON candidate array or a search envelope using TypeSafe.
    Rank {
        query: String,
        #[arg(long, default_value = "-")]
        input: String,
        #[arg(long)]
        share_content: bool,
    },
    /// Inspect Cider and credential availability without reading personal sources.
    Doctor,
    /// Configure a credential shared by tools and agents.
    Auth {
        #[command(subcommand)]
        command: AuthCmd,
    },
    /// List, inspect, or install the bundled portable agent skills.
    Skills {
        #[command(subcommand)]
        command: SkillsCmd,
    },
    /// Describe the machine-readable input/output contract.
    Schema,
}

#[derive(Args)]
struct SearchArgs {
    query: String,
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        default_value = "reminders,notes,safari"
    )]
    sources: Vec<Source>,
    /// Include files inside this directory using Spotlight.
    #[arg(long)]
    directory: Option<PathBuf>,
    #[arg(long)]
    list: Option<String>,
    #[arg(long, value_enum, default_value = "open")]
    status: Status,
    /// Read up to 30 note bodies instead of title/folder metadata.
    #[arg(long)]
    note_bodies: bool,
    #[arg(long, default_value_t = 10)]
    limit: usize,
    /// Rerank local candidates with TypeSafe; requires --share-content.
    #[arg(long, requires = "share_content")]
    ai: bool,
    /// Allow query, candidate titles and snippets to be sent to TypeSafe.
    #[arg(long, requires = "ai")]
    share_content: bool,
}

#[derive(Subcommand)]
enum AuthCmd {
    Status,
    /// Store a key with private file permissions. Default input is a hidden prompt.
    Set {
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum SkillsCmd {
    List,
    Show {
        name: String,
    },
    Install {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        force: bool,
    },
}

fn read_candidates(input: &str) -> Result<Vec<Candidate>> {
    let mut bytes = Vec::new();
    let reader: Box<dyn Read> = if input == "-" {
        Box::new(std::io::stdin())
    } else {
        Box::new(std::fs::File::open(input).context("cannot open candidate file")?)
    };
    reader.take(4_194_305).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 4_194_304, "candidate input exceeds 4 MiB");
    let mut value: Value =
        serde_json::from_slice(&bytes).context("candidate input must be JSON")?;
    if value.is_object() {
        value = value
            .get_mut("results")
            .context("candidate envelope has no results array")?
            .take();
    }
    let items: Vec<Candidate> =
        serde_json::from_value(value).context("invalid candidate array; run cider-ai schema")?;
    api::validate_candidates(&items)?;
    Ok(items)
}

async fn do_search(cli: &Cli, args: &SearchArgs, budget: Option<usize>) -> Result<Value> {
    let mut sources = args.sources.clone();
    if args.directory.is_some() && !sources.contains(&Source::Files) {
        sources.push(Source::Files);
    }
    ensure!(
        args.list.is_none() || sources.contains(&Source::Reminders),
        "--list requires the reminders source"
    );
    if let Some(budget) = budget {
        ensure!(
            (256..=100_000).contains(&budget),
            "context budget must be 256–100000 bytes"
        );
    }
    let mut result = search::search(search::SearchOptions {
        query: args.query.clone(),
        sources,
        directory: args.directory.clone(),
        list: args.list.clone(),
        status: args.status,
        note_bodies: args.note_bodies,
        limit: args.limit,
        cider: cli.cider.clone(),
        timeout: Duration::from_secs(cli.timeout),
    })
    .await?;
    let all_failed = result.sources.iter().all(|s| !s.ok);
    let mut ai_status = json!({"requested":args.ai,"applied":false});
    if args.ai && !result.results.is_empty() {
        match api::rank(
            &args.query,
            &mut result.results,
            &cli.model,
            Duration::from_secs(cli.timeout),
        )
        .await
        {
            Ok(meta) => {
                result.mode = "typesafe_reranked".into();
                ai_status = json!({"requested":true,"applied":true,"metadata":meta});
            }
            Err(error) => {
                result.partial = true;
                ai_status = json!({"requested":true,"applied":false,"error":error.to_string(),"fallback":"local lexical order retained"});
            }
        }
    }
    let available = result.results.len();
    let context = if let Some(budget) = budget {
        let (items, bytes) = search::bounded_context(&result.results, budget)?;
        result.results = items;
        json!({"budget_bytes":budget,"context_bytes":bytes,"selected":result.results.len(),"available":available,"unit":"UTF-8 JSON bytes of results array","omitted":available-result.results.len()})
    } else {
        Value::Null
    };
    let mut value = serde_json::to_value(result)?;
    value["schema_version"] = json!(1);
    value["ok"] = json!(!all_failed);
    value["ai"] = ai_status;
    if all_failed {
        value["error"] = json!({"code":"all_sources_failed","message":"No selected source could be read; inspect sources for details"});
    }
    if budget.is_some() {
        value["context"] = context;
    }
    Ok(value)
}

async fn run(cli: &Cli) -> Result<Value> {
    match &cli.command {
        Cmd::Search(args) => do_search(cli, args, None).await,
        Cmd::Context {
            search,
            budget_bytes,
        } => do_search(cli, search, Some(*budget_bytes)).await,
        Cmd::Rank {
            query,
            input,
            share_content,
        } => {
            ensure!(
                *share_content,
                "ranking sends query/title/snippets to TypeSafe; supply --share-content when authorized"
            );
            cider_ai::validate_query(query)?;
            let mut items = read_candidates(input)?;
            let meta = api::rank(
                query,
                &mut items,
                &cli.model,
                Duration::from_secs(cli.timeout),
            )
            .await?;
            Ok(
                json!({"schema_version":1,"ok":true,"query":query,"mode":"typesafe_reranked","api":meta,"results":items}),
            )
        }
        Cmd::Doctor => {
            let cider = search::run_cider(
                &cli.cider,
                &["schema".into(), "--source".into(), "reminders".into()],
                Duration::from_secs(cli.timeout),
            )
            .await;
            let auth = match credentials::load() {
                Ok(c) => json!({"configured":true,"source":c.source}),
                Err(e) => json!({"configured":false,"message":e.to_string()}),
            };
            Ok(
                json!({"schema_version":1,"ok":cider.is_ok(),"cider":{"available":cider.is_ok(),"error":cider.err().map(|e|e.to_string())},"typesafe":auth,"shared_key_path":credentials::shared_path()?.display().to_string(),"local_search_requires_api_key":false}),
            )
        }
        Cmd::Auth {
            command: AuthCmd::Status,
        } => {
            let status = match credentials::load() {
                Ok(c) => json!({"configured":true,"source":c.source}),
                Err(e) => json!({"configured":false,"message":e.to_string()}),
            };
            Ok(
                json!({"schema_version":1,"ok":true,"credential":status,"shared_key_path":credentials::shared_path()?.display().to_string(),"precedence":["TYPESAFE_API_KEY","TYPESAFE_API_KEY_FILE","shared file"]}),
            )
        }
        Cmd::Auth {
            command: AuthCmd::Set { stdin, force },
        } => {
            let key = if *stdin {
                credentials::from_stdin()?
            } else {
                rpassword::prompt_password("TypeSafe API key (hidden): ")?
            };
            let path = credentials::shared_path()?;
            credentials::save(&path, key, *force)?;
            Ok(
                json!({"schema_version":1,"ok":true,"stored":true,"path":path.display().to_string()}),
            )
        }
        Cmd::Skills {
            command: SkillsCmd::List,
        } => Ok(
            json!({"schema_version":1,"ok":true,"skills":skills::SKILLS.iter().map(|(name,_)|name).collect::<Vec<_>>()}),
        ),
        Cmd::Skills {
            command: SkillsCmd::Show { name },
        } => {
            let (_, body) = skills::SKILLS
                .iter()
                .find(|(n, _)| *n == name)
                .context("unknown skill")?;
            Ok(json!({"schema_version":1,"ok":true,"name":name,"content":body}))
        }
        Cmd::Skills {
            command: SkillsCmd::Install { dir, force },
        } => Ok(json!({"schema_version":1,"ok":true,"installed":skills::install(dir,*force)?})),
        Cmd::Schema => Ok(
            json!({"schema_version":1,"ok":true,"commands":["search","context","rank","doctor","auth status","auth set","skills list","skills show","skills install","schema"],"sources":["reminders","notes","safari","files"],"rank_input":{"type":"array","min_items":1,"max_items":50,"example":[{"id":"doc-1","title":"Notebook sharing","text":"Optional relevant excerpt","source":"alchemy","location":"optional local reference"}]},"result_fields":["id","title","text","source","location","modified","lexical_score","relevance"],"relevance":{"score":"0–3 relevance; higher is more relevant","confidence":"0–1 concentration, not correctness","probabilities":"distribution across 0,1,2,3"},"output":{"ok":"false and exit 1 on command failure; partial source failures retain successful results","partial":"inspect individual sources and ai for incomplete reads or ranking failure","schema_version":1},"context_budget":"UTF-8 bytes of serialized results array","privacy":{"default":"local only","remote_opt_in":"--ai --share-content for search/context; --share-content for rank","sent_fields":["query","title (300 chars)","text (1600 chars)"],"not_sent":["id","location","modified"]}}),
        ),
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let value = match run(&cli).await {
        Ok(value) => value,
        Err(error) => {
            json!({"schema_version":1,"ok":false,"error":{"code":"command_failed","message":error.to_string()}})
        }
    };
    let output = if cli.pretty {
        serde_json::to_string_pretty(&value)
    } else {
        serde_json::to_string(&value)
    };
    println!("{}", output.expect("JSON serialization"));
    if value["ok"] == false {
        std::process::exit(1);
    }
}
