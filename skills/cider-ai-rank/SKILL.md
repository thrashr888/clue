---
name: cider-ai-rank
description: Use when an agent already has candidate documents, tasks, source passages, or memory records and needs TypeSafe to rank their relevance to a specific request while preserving their IDs.
---

# Rank existing candidates

Use `cider-ai rank` on a JSON array of 1–50 candidates. Each requires a unique `id` and `title`; `text`, `source`, and `location` are optional. A previous cider-ai envelope with a `results` array is also accepted.

```sh
cider-ai rank "evidence about notebook sharing" --input candidates.json --share-content
```

`--input -` reads stdin, enabling pipelines. Confirm that sending the query and candidate titles/snippets to TypeSafe is within the user's authorization. IDs, locations, and modified timestamps stay local. Do not include credentials or unnecessary sensitive content in candidate text.

The CLI sends at most 300 title characters and 1,600 text characters per candidate, so include the relevant passage early. A score from 0 to 3 measures relevance; `confidence` measures concentration of the answer distribution, not relevance or truth. Preserve the original IDs and inspect uncertain results rather than treating them as verified facts.

Ranking can reorder retrieved candidates but cannot recover a missing candidate. If results are weak, improve retrieval before reranking more of the same. Missing/malformed answers fail the call without partially applying a ranking. Compare top results with a labeled baseline before claiming a retrieval improvement.

The binary reads `TYPESAFE_API_KEY`, then `TYPESAFE_API_KEY_FILE`, then the shared TypeSafe credential file. Use `cider-ai auth status` to inspect configuration without exposing the key. Do not put keys in command arguments, documents, or logs.
