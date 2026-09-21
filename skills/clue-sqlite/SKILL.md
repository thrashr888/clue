---
name: clue-sqlite
description: Use when an agent needs to inspect, search, or query a local SQLite database, or rank database records with TypeSafe while preserving their source IDs.
---

# Search SQLite with clue

Use the user's selected database. Start with `clue sqlite schema /absolute/path.db` to discover actual tables, column types, and primary keys. Do not guess private database schemas or timestamp meanings. Reads use an existing local file and a read-only connection, including its live WAL.

Search only relevant fields with a unique, non-null text/integer ID and text title column:

```sh
clue sqlite search /absolute/path.db "offline notebook sharing" --table issues --id-column id --title-column title --columns description,project
```

Local retrieval scans up to 1,000 rows ordered by the selected ID, configurable with `--scan-limit` from 1 to 5,000. It ranks lexical matches and returns up to `--limit` 1–50 candidates. Inspect `coverage`, `partial`, and `ai`. A truncated scan or empty shortlist does not establish absence. For large stores, use an indexed SQL filter or an existing database search facility to build the shortlist.

For exact filters, joins, and aggregates, use one SELECT/CTE. The CLI returns typed JSON rows and never executes generated model text:

```sh
clue sqlite query /absolute/path.db "SELECT project, count(*) AS open_count FROM issues WHERE status='open' GROUP BY project"
clue sqlite query /absolute/path.db "SELECT id, title, description AS text FROM issues WHERE project='Alchemy' ORDER BY id" --candidates
```

`--candidates` requires unique `id` and text `title` aliases; all selected fields form the bounded text snippet. Composite IDs can be constructed with SQL. Query candidate IDs are scoped to this database and query workflow, so namespace IDs in SQL when combining unrelated tables. Raw queries return at most 50 rows by default (up to 5,000), report `truncated`, and omit BLOB contents with an explicit marker. A 4 MiB output budget and 1 MiB SQLite value/row limit apply; reduce the projection when exceeded. Writes, attachments, extension loading, and unsupported functions are rejected. Use `--timeout` for the SQLite execution budget.

For semantic ranking, use `sqlite search ... --ai --share-content`, or pipe a `sqlite query ... --candidates` envelope into `clue rank "user question" --share-content`. Only do this when sharing the selected query/titles/snippets with TypeSafe is authorized. Credentials alone are not permission. Start API tests with `examples/projects.sql`, which contains synthetic data. Local database paths and IDs are not sent as separate API fields, but selected row contents can contain private information.

TypeSafe ranks retrieved evidence; it does not generate SQL or discover omitted records. Preserve returned IDs and inspect actual rows before making claims about the user's project. Never copy a live database alone while ignoring its WAL, modify the source, or use `immutable=1` to bypass locking.

For local AI ranking use `--provider ollama --model INSTALLED_MODEL`, or `--provider systemone --base-url http://127.0.0.1:8009 --model kev-latest`. Add `--ai` on search/context/SQLite search; `rank` always runs inference. These loopback paths need no `--share-content` and no TypeSafe credential. Environment defaults are `CLUE_PROVIDER`, `CLUE_MODEL`, `CLUE_BASE_URL`; use `CLUE_PROVIDER_API_KEY` only for a self-hosted server's own key. Non-loopback servers and Ollama cloud models require `--share-content`. Local proxy servers must be configured to keep inference local. Never silently fall back to a hosted provider.

Ollama returns `relevance.kind: generated_rating` without probabilities/confidence. Native System One results use `native_distribution`; confidence is not a cross-provider accuracy guarantee. Model availability, JSON validity, and ranking quality are separate checks. Start comparisons with synthetic `examples/candidates.json`; inspect `api` or `ai.metadata` for the provider actually used. Allow a longer `--timeout` for cold local inference (up to 600 seconds).

Inspect `clue config show` for saved/effective provider defaults before assuming a backend. Global defaults are set with `clue config set --provider NAME --model MODEL [--base-url URL]`. Flags override environment, then saved defaults; a provider override drops another provider's saved model/endpoint. Config never authorizes remote sharing. Consult the repository evaluation report before treating a model as reliable: successful HTTP/JSON validation is not a ranking-quality test.
