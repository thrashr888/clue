# cider-ai

One Rust CLI for local Mac search, bounded agent context, and optional TypeSafe relevance ranking. Cider reads local sources; cider-ai normalizes and searches them, preserves source references, and can ask Jev to rank a shortlist. Any agent or application that can run a command and parse JSON can use it.

This is a standalone project. It does not modify Cider, Alchemy, Cortex, or Shift and does not depend on the earlier Python experiment. Cider itself remains a runtime dependency for Mac sources; its adapters may have their own dependencies.

## Install

```sh
cargo install --path . --locked
cider-ai doctor
```

Requires Rust; Mac sources also require an installed `cider` command. Tested on macOS with Cider 0.6.2, Rust 1.98.1, and Jev 1.13.0. `CIDER_BIN` or `--cider` selects a particular Cider executable. SQLite and JSON candidate ranking can run without Cider; Mac source reads need macOS and its normal data permissions.

## Practical commands

```sh
# Search task titles and notes locally. No API key or network request.
cider-ai search "iPhone sync" --sources reminders --list Alchemy --pretty

# Include completed reminders, or select completed ones only.
cider-ai search "sync" --sources reminders --list Alchemy --status completed

# Search a selected repository through Spotlight.
cider-ai search "sandbox lifecycle" --sources files --directory /absolute/path/to/agentkernel

# Combine sources; partial failures remain visible.
cider-ai search "canvas" --sources reminders,notes,safari --limit 10

# Assemble a small evidence bundle for another agent.
cider-ai context "notebook sharing" --sources reminders,files \
  --list Alchemy --directory /absolute/path/to/project --budget-bytes 12000

# Rank existing candidates: this command explicitly shares title/snippet text.
cider-ai rank "sharing research notebooks between devices" \
  --input examples/candidates.json --share-content --pretty

# A search envelope can be piped into rank.
cider-ai search "sync" --sources reminders --list Alchemy |
  cider-ai rank "decisions about notebook sharing" --input - --share-content

# Or combine local search and external ranking in one call.
cider-ai search "notebook sharing" --sources notes --ai --share-content
```

Search queries are lexical terms, not a general natural-language command language. An agent uses the workflow skills to translate a request into suitable queries and flags. The earlier experiment's calendar/date planner has not been ported into this first release.

## Shared credentials

Credentials are resolved in this order:

1. `TYPESAFE_API_KEY`
2. The file named by `TYPESAFE_API_KEY_FILE`
3. `$XDG_CONFIG_HOME/typesafe/api-key`, or `~/.config/typesafe/api-key`

```sh
cider-ai auth set          # Hidden interactive prompt; saves mode 600
cider-ai auth status       # Reports source/path, never the key
cider-ai auth set --force  # Replace a stored key after rotation
```

`auth set --stdin` supports piping from a secret manager. Avoid putting keys in shell arguments or history. Existing keys are not overwritten without `--force`. Credential files accessible to other users are rejected on Unix. The default shared directory is mode 700. This is a private plaintext credential file, not macOS Keychain storage.

Local search and context do not require credentials. Cider subprocesses do not inherit `TYPESAFE_API_KEY` or `TYPESAFE_API_KEY_FILE`. `TYPESAFE_MODEL` or `--model` can override the default `jev-latest` model.

## Calendar

```sh
cider-ai search "planning" --sources calendar --days-back 0 --days-ahead 14
cider-ai search "review" --sources calendar --calendar Work --days-back 7 --days-ahead 30
cider-ai context "project launch" --sources calendar,reminders,notes --days-ahead 14 --budget-bytes 6000
```

Calendar is opt-in with `--sources calendar`; existing default sources are unchanged. Each date bound accepts 0–366 days relative to the current time. Cider filters by occurrence start, so this is not an overlap/free-busy query or a natural-language date parser. `event` preserves calendar name, start/end, all-day state, and physical location. Source timestamps are preserved without guessing timezones. Recurring instances retain separate IDs. Reading Calendar requires the same macOS permissions as Cider.

## SQLite: project data as searchable evidence

SQLite works independently of Cider and macOS. Start with the schema, select a table and fields, then search locally:

```sh
sqlite3 /tmp/cider-ai-demo.db < examples/projects.sql
cider-ai sqlite schema /tmp/cider-ai-demo.db --pretty
cider-ai sqlite search /tmp/cider-ai-demo.db "offline notebook sharing" --table issues --columns description,project --pretty
cider-ai sqlite query /tmp/cider-ai-demo.db "SELECT project, count(*) AS open_count FROM issues WHERE status='open' GROUP BY project"
```

The fixture contains **synthetic** Alchemy, Cider, Cortex, and AgentKernel issues. Useful real applications include finding related Alchemy sources, recalling Cortex decisions, searching exported issue databases, or analyzing test/run records. Point the CLI at a selected database and inspect its schema first; these examples do not establish integrations with those apps.

Search defaults to `id` and `title`; override with `--id-column` and `--title-column`. IDs must be unique, non-null text/integers and titles must be text. `--columns` chooses additional searchable fields. A Unicode-aware lexical scan reads at most 1,000 rows ordered by ID (`--scan-limit` 1–5,000). `coverage.truncated` and `partial` identify incomplete scans. This is designed for bounded evidence retrieval; use SQL filters/indexes for large databases.

TypeSafe can rank the shortlist when sharing is authorized:

```sh
# Synthetic data: safe to use for an API smoke test.
cider-ai sqlite search /tmp/cider-ai-demo.db "offline notebook sharing" --table issues --columns description,project --ai --share-content

# SQL chooses candidates; TypeSafe judges their relevance.
cider-ai sqlite query /tmp/cider-ai-demo.db "SELECT i.id, i.title, i.description AS text, p.language FROM issues i JOIN projects p ON p.name=i.project WHERE i.status='open' ORDER BY i.id" --candidates | cider-ai rank "keep offline notebook edits consistent across my devices" --share-content
```

TypeSafe reranks rows rather than generating SQL. `query --candidates` requires `id` and `title` aliases, includes selected fields in the snippet, and produces the same envelope accepted by `rank`. For a composite key, construct a unique text ID in SQL. Query candidate IDs include the database path and a `query` scope; namespace IDs yourself when merging queries from unrelated tables.

Queries use one SELECT/CTE, a read-only connection, `query_only`, an authorizer, and a small allowlist of common read functions. Writes, attachments, extensions, and arbitrary functions are rejected. Live WAL records remain visible; no immutable shortcut is used. SQL execution has a progress-handler deadline (`--timeout`), while lock waits are capped at one second. These are operational bounds, not a sandbox for adversarial databases.

Raw query results preserve text, numbers, and nulls; BLOBs become explicit size/omission markers. `--limit` defaults to 50, supports 1–5,000 raw rows or 1–50 candidates, and reports `truncated`. SQLite values/rows are capped at 1 MiB and accumulated row JSON at 4 MiB; excess produces an error rather than silently losing fields. Candidate snippets are capped at 1,600 characters. Custom extensions, app-specific collations, and encrypted databases are unsupported.

## Agent skills

Four portable skills are compiled into the binary and also available in `skills/`:

| Skill | Workflow |
| --- | --- |
| `cider-ai-search` | Find local tasks, notes, calendar events, browsing history, and files |
| `cider-ai-sqlite` | Inspect schemas, search selected fields, run read-only SQL, and rank rows |
| `cider-ai-context` | Resume a project with a bounded evidence bundle |
| `cider-ai-rank` | Rank externally supplied documents, passages, tasks, or memories |

```sh
cider-ai skills list
cider-ai skills show cider-ai-search

# Codex, globally:
cider-ai skills install --dir ~/.codex/skills

# Claude Code, globally:
cider-ai skills install --dir ~/.claude/skills

# Project-local agents using the shared skills convention:
cider-ai skills install --dir /absolute/project/.agents/skills
```

Installation checks every destination before writing and refuses to replace changed skills unless `--force` is supplied. No plugin or MCP server is needed: the skills call the binary. Other agents can use the JSON CLI directly without installing a skill.

## Machine interface

All operational commands emit one JSON object on stdout. Help/version are plain text. Use global `--pretty` for inspection. `cider-ai schema` describes inputs, outputs, supported sources, and privacy behavior.

Every envelope includes `schema_version: 1` and `ok`. Failures use `ok: false` and exit status 1. Bad CLI syntax exits 2. A search with some failed sources retains successful results, sets `partial: true`, and exits 0; callers must inspect `sources` and `ai`. If every source fails, the command exits 1 with source diagnostics intact.

Candidates carry stable `id`, `source`, `title`, bounded `text`, optional `location`, `modified`, and Calendar `event` metadata, `lexical_score`, and optional `relevance`. Reminder/Note IDs retain Cider's identity with a source prefix; URLs and file paths remain local source references. To fetch full records, remove the source prefix and use the corresponding Cider read command. The CLI never opens a source, executes a suggested command, or changes user data automatically.

`rank` accepts either a candidate array or a previous search/context envelope. Each candidate needs a unique `id` and `title`; see [the example](examples/candidates.json). Inputs are limited to 50 candidates and 4 MiB. API scoring preserves input IDs and validates complete probability distributions before applying any ordering.

Context budgets measure the compact UTF-8 JSON encoding of the returned `results` array. They are not token estimates and do not include envelope metadata. Oversized records are omitted; `context.available`, `selected`, `omitted`, and `context_bytes` make that visible.

## Data flow and limits

Search is local by default. External ranking requires `--share-content`; it sends only the query and each candidate's first 300 title characters and 1,600 text characters. IDs, source paths/URLs in the `location` field, and timestamps are not sent as separate fields, but such information may still occur within the title or snippet itself. This is field minimization, not automatic sensitive-data redaction.

Local search uses weighted substring matching over at most 12 query terms. TypeSafe reranks the candidates it receives; it cannot recover items that local retrieval missed. No persistent index or embeddings are built in this version.

| Source | Current coverage |
| --- | --- |
| Calendar | Up to 500 occurrences starting in a rolling window, default 7 days back / 30 ahead. Supports calendar name filtering. |
| Reminders | Bounded Cider read, up to 500 rows; some stores cap before list filtering. Supports list and open/completed/all status. |
| Notes | Up to 200 titles/folders by default; `--note-bodies` reads a sample of up to 30 bodies. |
| Safari | Up to 300 recent history entries. Bookmarks and other browsers are not included yet. |
| Files | Up to 200 Spotlight candidates within the selected directory. Text previews read at most 64 KiB for selected text/code formats. Symlinks outside the directory are excluded. |

Empty results do not establish that the full store has no matches. Search order is not manual task priority. File retrieval depends on Spotlight indexing. macOS sandbox/TCC restrictions can cause individual sources to fail even when they work in Terminal.

The default per-source and HTTP timeout is 15 seconds, configurable from 1 to 60 with `--timeout`. Search sources run concurrently. API calls are not retried automatically. If optional search reranking fails, the CLI retains the local ordering and explicitly reports that fallback. Standalone `rank` fails instead of claiming a ranking was applied. Relevance scores range from 0 to 3; confidence describes answer-distribution concentration, not correctness.

## Verification

- 22 unit/integration tests cover credentials, source errors, recurring Calendar occurrences, date-window arguments, read-only SQL enforcement, live WAL reads, query limits, Unicode, joins, IDs, ranking validation, context bounds, and skill installation.
- Calendar live smoke: 40 occurrences read, 2 keyword matches with structured event metadata.
- SQLite smoke: schema discovery, joins, counts, lexical search, and unchanged database bytes. A synthetic four-row TypeSafe query ranked Cortex decision retrieval first at 2.99/3 (290 ms); no personal rows were sent.
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` pass.
- All four SKILL.md files pass the skill validator.
- Real local reads found three Alchemy sync reminders, three Notes title matches, and five displayed AgentKernel file matches. A context bundle used 2,978 of 4,000 available bytes.
- One synthetic four-candidate API check resolved to `jev-1.13.0`, ranked notebook sharing first at 2.98/3, and took 262 ms with 899 input and 60 output tokens. This is a smoke check, not a benchmark or latency guarantee.

Private live search results are excluded from Git. See [validation.json](validation.json) for the recorded non-content checks.

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

## Integration direction

Alchemy can supply search candidates to `rank`; Cortex can supply memory candidates and consume bounded context; Shift can invoke the skills or parse JSON directly. These are integration points, not existing connections to those projects. The same interface can support future capture routing, source-change classification, or related-work detection without requiring a separate agent runtime.

TypeSafe's [HTTP API](https://docs.typesafe.ai/api), [Score primitive](https://docs.typesafe.ai/primitives/score), and [reranking cookbook](https://docs.typesafe.ai/cookbooks/rerank_typesafe) informed the integration. The TypeSafe development skill is included under `.agents/skills/typesafe-ai/` with its upstream license.
