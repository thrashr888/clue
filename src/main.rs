use anyhow::{Context, Result, ensure};
use clap::{Args, Parser, Subcommand};
use clue::{
    Candidate, config, credentials, input, provider,
    search::{self, Source, Status},
    skills, sqlite,
};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(
    version,
    about = "Semantic ranking for CLI output, local search, and bounded agent context"
)]
struct Cli {
    #[arg(long, global = true)]
    pretty: bool,
    #[arg(long, global = true, env = "CIDER_BIN", default_value = "cider")]
    cider: PathBuf,
    #[command(flatten)]
    provider: provider::Selection,
    #[arg(long,global=true,default_value_t=15,value_parser=clap::value_parser!(u64).range(1..=600))]
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
    /// Rank a JSON candidate array or a search envelope using the selected provider.
    Rank {
        query: String,
        #[command(flatten)]
        input: input::InputArgs,
        #[arg(long)]
        share_content: bool,
    },
    /// Collect JSON/JSONL records locally, preserving originals; never calls a ranking provider.
    Collect {
        #[command(flatten)]
        input: input::InputArgs,
    },
    /// Assemble a bounded context bundle from records supplied by any tool.
    Bundle {
        #[command(flatten)]
        input: input::InputArgs,
        #[arg(long, default_value_t = 12000)]
        budget_bytes: usize,
    },
    /// List or inspect reusable command and field-mapping profiles.
    Profiles {
        #[command(subcommand)]
        command: ProfileCmd,
    },
    /// Inspect or search a SQLite database using a read-only connection.
    Sqlite {
        #[command(subcommand)]
        command: SqliteCmd,
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
    /// Manage global provider/model defaults. Never stores keys or sharing consent.
    Config {
        #[command(subcommand)]
        command: ConfigCmd,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Show the saved and effective defaults, including environment/flag overrides.
    Show,
    /// Save --provider, --model, and optional --base-url as global defaults.
    Set,
    /// Remove saved defaults; credentials remain separate.
    Reset,
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
    /// Filter Calendar events by calendar name (requires calendar source).
    #[arg(long)]
    calendar: Option<String>,
    #[arg(long, default_value_t = 7, value_parser = clap::value_parser!(u32).range(0..=366))]
    days_back: u32,
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(0..=366))]
    days_ahead: u32,
    #[arg(long, value_enum, default_value = "open")]
    status: Status,
    /// Read up to 30 note bodies instead of title/folder metadata.
    #[arg(long)]
    note_bodies: bool,
    #[arg(long, default_value_t = 10)]
    limit: usize,
    /// Rerank local candidates with the selected provider; remote inference requires --share-content.
    #[arg(long)]
    ai: bool,
    /// Allow query, candidate titles and snippets to be sent to a remote provider.
    #[arg(long, requires = "ai")]
    share_content: bool,
}

#[derive(Subcommand)]
enum SqliteCmd {
    /// List tables and columns without reading row contents.
    Schema { database: PathBuf },
    /// Search selected fields in a bounded table scan; optionally rerank with the selected provider.
    Search {
        database: PathBuf,
        query: String,
        #[arg(long)]
        table: String,
        #[arg(long, default_value = "id")]
        id_column: String,
        #[arg(long, default_value = "title")]
        title_column: String,
        #[arg(long, value_delimiter = ',', required = true)]
        columns: Vec<String>,
        #[arg(long, default_value_t = 1000)]
        scan_limit: usize,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        ai: bool,
        #[arg(long, requires = "ai")]
        share_content: bool,
    },
    /// Run a single SELECT/CTE. Alias id/title/text and use --candidates to pipe to rank.
    Query {
        database: PathBuf,
        sql: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        candidates: bool,
    },
}

#[derive(Subcommand)]
enum ProfileCmd {
    List,
    Show { name: String },
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

async fn do_search(cli: &Cli, args: &SearchArgs, budget: Option<usize>) -> Result<Value> {
    let ranking = args
        .ai
        .then(|| cli.provider.resolve(args.share_content))
        .transpose()?;
    let mut sources = args.sources.clone();
    if args.directory.is_some() && !sources.contains(&Source::Files) {
        sources.push(Source::Files);
    }
    ensure!(
        args.list.is_none() || sources.contains(&Source::Reminders),
        "--list requires the reminders source"
    );
    ensure!(
        args.calendar.is_none() || sources.contains(&Source::Calendar),
        "--calendar requires the calendar source"
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
        calendar: args.calendar.clone(),
        days_back: args.days_back,
        days_ahead: args.days_ahead,
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
        match ranking
            .as_ref()
            .unwrap()
            .rank(
                &args.query,
                &mut result.results,
                Duration::from_secs(cli.timeout),
                args.share_content,
            )
            .await
        {
            Ok(meta) => {
                result.mode = ranking.as_ref().unwrap().mode();
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
        Cmd::Config { command } => {
            let path = config::path()?;
            match command {
                ConfigCmd::Show => {
                    let saved = config::load()?;
                    let effective = cli.provider.effective(saved.as_ref()).resolve(true)?;
                    Ok(
                        json!({"schema_version":1,"ok":true,"path":path,"saved":saved,"effective":effective.description(),"precedence":["flags","environment","matching-provider config","built-in defaults"],"sharing_authorized":false}),
                    )
                }
                ConfigCmd::Set => {
                    let provider = cli
                        .provider
                        .provider
                        .context("config set requires --provider and --model")?;
                    let model = cli
                        .provider
                        .model
                        .clone()
                        .context("config set requires --provider and --model")?;
                    let saved = config::Saved {
                        provider,
                        model: Some(model),
                        base_url: cli.provider.base_url.clone(),
                    };
                    config::save(&path, &saved)?;
                    Ok(json!({"schema_version":1,"ok":true,"path":path,"saved":saved}))
                }
                ConfigCmd::Reset => {
                    config::reset()?;
                    Ok(json!({"schema_version":1,"ok":true,"path":path,"reset":true}))
                }
            }
        }
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
            let ranking = cli.provider.resolve(*share_content)?;
            clue::validate_query(query)?;
            let mut collected = input::collect(input, Duration::from_secs(cli.timeout)).await?;
            let meta = ranking
                .rank(
                    query,
                    &mut collected.items,
                    Duration::from_secs(cli.timeout),
                    *share_content,
                )
                .await?;
            let mut output = collected.envelope();
            output["mode"] = json!(ranking.mode());
            output["query"] = json!(query);
            output["api"] = serde_json::to_value(meta)?;
            Ok(output)
        }
        Cmd::Bundle {
            input,
            budget_bytes,
        } => {
            ensure!(
                (256..=100_000).contains(budget_bytes),
                "context budget must be 256–100000 bytes"
            );
            let mut collected = input::collect(input, Duration::from_secs(cli.timeout)).await?;
            let available = collected.items.len();
            let (items, bytes) = search::bounded_context(&collected.items, *budget_bytes)?;
            collected.items = items;
            let mut output = collected.envelope();
            output["mode"] = json!("bounded_context");
            output["context"] = json!({"budget_bytes":budget_bytes,"context_bytes":bytes,"available":available,"selected":collected.items.len(),"omitted":available-collected.items.len(),"unit":"UTF-8 JSON bytes of results array"});
            Ok(output)
        }
        Cmd::Collect { input } => Ok(input::collect(input, Duration::from_secs(cli.timeout))
            .await?
            .envelope()),
        Cmd::Profiles {
            command: ProfileCmd::List,
        } => Ok(
            json!({"schema_version":1,"ok":true,"profiles":input::PROFILES.iter().map(|(n,_)|n).collect::<Vec<_>>()}),
        ),
        Cmd::Profiles {
            command: ProfileCmd::Show { name },
        } => Ok(json!({"schema_version":1,"ok":true,"profile":input::profile(name)?})),
        Cmd::Sqlite { command } => {
            let ranking = if let SqliteCmd::Search {
                ai: true,
                share_content,
                ..
            } = command
            {
                Some(cli.provider.resolve(*share_content)?)
            } else {
                None
            };
            let path = match command {
                SqliteCmd::Schema { database }
                | SqliteCmd::Search { database, .. }
                | SqliteCmd::Query { database, .. } => database,
            };
            let db = sqlite::Database::open(path, Duration::from_secs(cli.timeout))?;
            match command {
                SqliteCmd::Schema { .. } => db.schema(),
                SqliteCmd::Query {
                    sql,
                    limit,
                    candidates,
                    ..
                } => {
                    ensure!(!candidates || *limit <= 50, "candidate limit must be 1–50");
                    let rows = db.query(sql, *limit)?;
                    let mut output = json!({"schema_version":1,"ok":true,"database":db.path(),"read_only":true,"truncated":rows.truncated});
                    if *candidates {
                        let mut items = db.candidates(&rows.values, "id", "title", "query")?;
                        for item in &mut items {
                            item.text = clue::clip(&item.text, 1600);
                        }
                        output["results"] = serde_json::to_value(items)?;
                    } else {
                        output["rows"] = json!(rows.values);
                    }
                    Ok(output)
                }
                SqliteCmd::Search {
                    query,
                    table,
                    id_column,
                    title_column,
                    columns,
                    scan_limit,
                    limit,
                    ai,
                    share_content,
                    ..
                } => {
                    let mut output = db.search(
                        query,
                        &sqlite::SearchOptions {
                            table: table.clone(),
                            id_column: id_column.clone(),
                            title_column: title_column.clone(),
                            columns: columns.clone(),
                            scan_limit: *scan_limit,
                            limit: *limit,
                        },
                    )?;
                    output["ai"] = json!({"requested":ai,"applied":false});
                    if *ai && !output["results"].as_array().unwrap().is_empty() {
                        let mut items: Vec<Candidate> =
                            serde_json::from_value(output["results"].take())?;
                        match ranking
                            .as_ref()
                            .unwrap()
                            .rank(
                                query,
                                &mut items,
                                Duration::from_secs(cli.timeout),
                                *share_content,
                            )
                            .await
                        {
                            Ok(meta) => {
                                output["mode"] = json!(ranking.as_ref().unwrap().mode());
                                output["ai"] =
                                    json!({"requested":true,"applied":true,"metadata":meta});
                            }
                            Err(error) => {
                                output["partial"] = json!(true);
                                output["ai"] = json!({"requested":true,"applied":false,"error":error.to_string(),"fallback":"local lexical order retained"});
                            }
                        }
                        output["results"] = serde_json::to_value(items)?;
                    }
                    Ok(output)
                }
            }
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
            json!({"schema_version":1,"ok":true,"config":{"path":"$XDG_CONFIG_HOME/clue/config.json or ~/.config/clue/config.json","override_path":"CLUE_CONFIG (absolute path)","precedence":["flags","environment","matching-provider config","built-in defaults"],"stores":["provider","model","base_url"],"credentials_and_sharing":"never stored"},"commands":["config show","config set","config reset","search","context","collect","bundle","rank","profiles list","profiles show","sqlite schema","sqlite search","sqlite query","doctor","auth status","auth set","skills list","skills show","skills install","schema"],"providers":["typesafe","systemone","ollama"],"sources":["reminders","notes","safari","files","calendar"],"rank_input":{"formats":["JSON array","JSONL","results envelope","single object"],"mapping":"--id/--title/--text/--ref accept field names or JSON Pointer paths; --profile applies saved mappings","execution":"explicit argv after -- or --run-profile; otherwise stdin/file","max_bytes":4194304,"type":"array","min_items":1,"max_items":50,"example":[{"id":"doc-1","title":"Notebook sharing","text":"Optional relevant excerpt","source":"alchemy","location":"optional local reference"}]},"result_fields":["id","title","text","source","location","modified","event","lexical_score","relevance","record"],"relevance":{"score":"0–3 relevance; higher is more relevant","kind":"native_distribution or generated_rating","confidence":"optional; native 0–1 concentration, not correctness","probabilities":"optional; native distribution across 0,1,2,3; omitted for Ollama"},"output":{"ok":"false and exit 1 on command failure; partial source failures retain successful results","partial":"inspect individual sources and ai for incomplete reads or ranking failure","schema_version":1},"sqlite":{"commands":["schema","search","query"],"search":"explicit table/columns; bounded ID-ordered scan","query":"single read-only SELECT/CTE; --candidates requires id/title aliases","max_scan_rows":5000,"max_candidates":50,"max_raw_rows":5000,"max_row_json_bytes":4194304,"blob_contents":"omitted with size marker","ai":"optional reranking with --ai --share-content or query --candidates piped to rank"},"calendar":{"source":"opt-in","default_days_back":7,"default_days_ahead":30,"filter":"occurrence start; rolling window","timestamps":"preserved from Cider without inferred timezone"},"context_budget":"UTF-8 bytes of serialized results array","privacy":{"default":"local only","remote_opt_in":"--share-content for remote providers or Ollama cloud models; loopback ranking does not require it","sent_fields":["query","title (300 chars)","text (1600 chars)"],"not_sent":["id","location","modified","event","record"]}}),
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
