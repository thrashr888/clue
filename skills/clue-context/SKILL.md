---
name: clue-context
description: Use when resuming a project or preparing a bounded evidence bundle for an agent from local project files, reminders, Apple Notes, Calendar events, and Safari history.
---

# Prepare project context

Use `clue context` to retrieve an evidence bundle, then read the returned records before explaining the next step.

```sh
clue context "notebook sharing" --sources reminders,files --list Alchemy --directory /absolute/project/path --budget-bytes 12000
```

Use the project and scope the user supplied. If a repository path is unknown, find it or ask rather than guessing. Queries use lexical terms: choose distinctive task names or concepts and split unrelated topics into separate calls. There is no implicit Git, Cortex, or Alchemy database integration in this version.

The budget covers the UTF-8 JSON encoding of the returned `results` array, not tokens or the whole envelope. `context_bytes` is measured. Oversized records may be omitted; compare `selected` with `available` and retain links/IDs for a later focused read. Do not copy giant artifacts into agent context.

For meeting preparation, include `--sources calendar,reminders,notes` with explicit `--days-back` and `--days-ahead` bounds. Calendar retains structured start/end/all-day metadata; date words are not parsed into filters. Use the separate `clue-sqlite` workflow for a database the user selects.

Inspect source coverage and errors. Notes default to title/folder search; `--note-bodies` reads a bounded sample of note contents. Search ranking does not establish freshness, task priority, completion of an implementation, or that a release shipped. Verify such claims against the relevant source or repository.

Write a brief handoff based on returned evidence: objective, relevant decisions/tasks, known gaps, and the next concrete step. Keep that synthesis distinct from literal source excerpts. `--ai --share-content` is optional external reranking and requires authorization to send query/title/snippets; local context requires no API credential.

For local AI ranking use `--provider ollama --model INSTALLED_MODEL`, or `--provider systemone --base-url http://127.0.0.1:8009 --model kev-latest`. Add `--ai` on search/context/SQLite search; `rank` always runs inference. These loopback paths need no `--share-content` and no TypeSafe credential. Environment defaults are `CLUE_PROVIDER`, `CLUE_MODEL`, `CLUE_BASE_URL`; use `CLUE_PROVIDER_API_KEY` only for a self-hosted server's own key. Non-loopback servers and Ollama cloud models require `--share-content`. Local proxy servers must be configured to keep inference local. Never silently fall back to a hosted provider.

Ollama returns `relevance.kind: generated_rating` without probabilities/confidence. Native System One results use `native_distribution`; confidence is not a cross-provider accuracy guarantee. Model availability, JSON validity, and ranking quality are separate checks. Start comparisons with synthetic `examples/candidates.json`; inspect `api` or `ai.metadata` for the provider actually used. Allow a longer `--timeout` for cold local inference (up to 600 seconds).
