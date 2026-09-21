---
name: clue-search
description: Use when the user wants to find something on their Mac across reminders, Apple Notes, Calendar events, Safari history, or files in a selected folder, including a document or task they remember by subject rather than exact title.
---

# Search the Mac with clue

Use the `clue` binary. It emits versioned JSON on stdout; use `clue schema` to inspect the contract. It wraps installed Cider read commands and does not modify user data.

Start locally with a short set of distinctive search words, preserving names and dates from the user's request:

```sh
clue search "iPhone sync" --sources reminders --list Alchemy
clue search "sandbox lifecycle" --sources files --directory /absolute/project/path
clue search "planning" --sources calendar --calendar Work --days-back 0 --days-ahead 14
clue search "canvas" --sources reminders,notes,safari --limit 10
```

Search is lexical candidate retrieval, not a general natural-language planner. Do not imply that words such as "yesterday" become date filters. Use `--status completed` for finished reminders, `--status all` for both states. Notes are title/folder-only unless `--note-bodies` is supplied. Files require a selected directory and depend on Spotlight indexing.

Calendar uses a rolling start-date window (default 7 days back, 30 ahead; each 0–366). It is not a day-boundary or availability query. Use explicit `--days-back`, `--days-ahead`, and optional `--calendar`; date words in the query remain lexical terms. Preserve `event` start/end/all-day metadata exactly, including timestamps without a timezone suffix; do not invent timezone conversions. Recurring occurrences have distinct candidate IDs.

Inspect `sources`, `partial`, and each source's `coverage`. A failed source is not an empty source. Empty bounded reads do not prove absence from the full store. Return the actual matching titles and source IDs/locations, keeping uncertain interpretations separate from observed matches. Do not claim search order is the user's manual priority order.

Optional `--ai --share-content` reranks the shortlist with TypeSafe. It sends the query, titles and bounded text snippets to an external API. Use it only when this sharing is authorized; credentials alone are not consent. API failure retains local results and reports the failure in `ai`. Do not call those results AI-ranked.

Use `clue doctor` for capability and credential diagnostics. For permission failures, explain the affected source and use the host's normal approval mechanism; do not change macOS privacy settings automatically. Never print or ask the user to put an API key in a command argument.

For local AI ranking use `--provider ollama --model INSTALLED_MODEL`, or `--provider systemone --base-url http://127.0.0.1:8009 --model kev-latest`. Add `--ai` on search/context/SQLite search; `rank` always runs inference. These loopback paths need no `--share-content` and no TypeSafe credential. Environment defaults are `CLUE_PROVIDER`, `CLUE_MODEL`, `CLUE_BASE_URL`; use `CLUE_PROVIDER_API_KEY` only for a self-hosted server's own key. Non-loopback servers and Ollama cloud models require `--share-content`. Local proxy servers must be configured to keep inference local. Never silently fall back to a hosted provider.

Ollama returns `relevance.kind: generated_rating` without probabilities/confidence. Native System One results use `native_distribution`; confidence is not a cross-provider accuracy guarantee. Model availability, JSON validity, and ranking quality are separate checks. Start comparisons with synthetic `examples/candidates.json`; inspect `api` or `ai.metadata` for the provider actually used. Allow a longer `--timeout` for cold local inference (up to 600 seconds).

Inspect `clue config show` for saved/effective provider defaults before assuming a backend. Global defaults are set with `clue config set --provider NAME --model MODEL [--base-url URL]`. Flags override environment, then saved defaults; a provider override drops another provider's saved model/endpoint. Config never authorizes remote sharing. Consult the repository evaluation report before treating a model as reliable: successful HTTP/JSON validation is not a ranking-quality test.
