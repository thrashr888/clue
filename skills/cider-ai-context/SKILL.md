---
name: cider-ai-context
description: Use when resuming a project or preparing a bounded evidence bundle for an agent from local project files, reminders, Apple Notes, and Safari history.
---

# Prepare project context

Use `cider-ai context` to retrieve an evidence bundle, then read the returned records before explaining the next step.

```sh
cider-ai context "notebook sharing" --sources reminders,files --list Alchemy --directory /absolute/project/path --budget-bytes 12000
```

Use the project and scope the user supplied. If a repository path is unknown, find it or ask rather than guessing. Queries use lexical terms: choose distinctive task names or concepts and split unrelated topics into separate calls. There is no implicit Git, Cortex, or Alchemy database integration in this version.

The budget covers the UTF-8 JSON encoding of the returned `results` array, not tokens or the whole envelope. `context_bytes` is measured. Oversized records may be omitted; compare `selected` with `available` and retain links/IDs for a later focused read. Do not copy giant artifacts into agent context.

Inspect source coverage and errors. Notes default to title/folder search; `--note-bodies` reads a bounded sample of note contents. Search ranking does not establish freshness, task priority, completion of an implementation, or that a release shipped. Verify such claims against the relevant source or repository.

Write a brief handoff based on returned evidence: objective, relevant decisions/tasks, known gaps, and the next concrete step. Keep that synthesis distinct from literal source excerpts. `--ai --share-content` is optional external reranking and requires authorization to send query/title/snippets; local context requires no API credential.
