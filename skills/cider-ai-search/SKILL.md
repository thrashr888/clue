---
name: cider-ai-search
description: Use when the user wants to find something on their Mac across reminders, Apple Notes, Calendar events, Safari history, or files in a selected folder, including a document or task they remember by subject rather than exact title.
---

# Search the Mac with cider-ai

Use the `cider-ai` binary. It emits versioned JSON on stdout; use `cider-ai schema` to inspect the contract. It wraps installed Cider read commands and does not modify user data.

Start locally with a short set of distinctive search words, preserving names and dates from the user's request:

```sh
cider-ai search "iPhone sync" --sources reminders --list Alchemy
cider-ai search "sandbox lifecycle" --sources files --directory /absolute/project/path
cider-ai search "planning" --sources calendar --calendar Work --days-back 0 --days-ahead 14
cider-ai search "canvas" --sources reminders,notes,safari --limit 10
```

Search is lexical candidate retrieval, not a general natural-language planner. Do not imply that words such as "yesterday" become date filters. Use `--status completed` for finished reminders, `--status all` for both states. Notes are title/folder-only unless `--note-bodies` is supplied. Files require a selected directory and depend on Spotlight indexing.

Calendar uses a rolling start-date window (default 7 days back, 30 ahead; each 0–366). It is not a day-boundary or availability query. Use explicit `--days-back`, `--days-ahead`, and optional `--calendar`; date words in the query remain lexical terms. Preserve `event` start/end/all-day metadata exactly, including timestamps without a timezone suffix; do not invent timezone conversions. Recurring occurrences have distinct candidate IDs.

Inspect `sources`, `partial`, and each source's `coverage`. A failed source is not an empty source. Empty bounded reads do not prove absence from the full store. Return the actual matching titles and source IDs/locations, keeping uncertain interpretations separate from observed matches. Do not claim search order is the user's manual priority order.

Optional `--ai --share-content` reranks the shortlist with TypeSafe. It sends the query, titles and bounded text snippets to an external API. Use it only when this sharing is authorized; credentials alone are not consent. API failure retains local results and reports the failure in `ai`. Do not call those results AI-ranked.

Use `cider-ai doctor` for capability and credential diagnostics. For permission failures, explain the affected source and use the host's normal approval mechanism; do not change macOS privacy settings automatically. Never print or ask the user to put an API key in a command argument.
