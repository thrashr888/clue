# Ranking evaluation

These are end-to-end Clue runs against real model servers, not mocked HTTP tests. The fixed fixture has 12 assistant-authored synthetic scenarios inspired by developer workflows, with four graded records each. Labels were written before inference and never sent to models. They are not independent human annotations or a representative production benchmark.

Each scenario runs in its original order, a one-position rotation, and reverse order: 36 requests per completed model run. Eleven scenarios have a best result (33 top-result checks); one has no relevant result. Scenarios include negation, exact project identity, Spanish, SQLite/WAL correctness, privacy, and an adversarial record asking the evaluator to override its rubric.

Before running models, provisional gates were fixed at >=90% top-result accuracy, >=0.90 mean nDCG@3, zero API/validation errors, no-match maximum score <=1 in all three orders, and a correct winner in every adversarial order. Passing these gates is evidence only for this fixture. It does not establish production reliability, calibration, tool-execution safety, or untested model variants.

`results/` contains raw per-request ranks, scores, errors, and timing. `provenance.json` records server/checkpoint revisions and device details. Quality runs overlapped with model startup and other inference, so their wall times are diagnostic observations, not controlled comparative latency measurements.

## Results (2026-09-21)

| Model / evaluated runtime | Correct top result | Mean nDCG@3 | Adversarial orders | No-match orders | Provisional gates |
| --- | ---: | ---: | --- | --- | --- |
| Jev (`jev-latest`, TypeSafe) | 33/33 (100%) | 0.994 | 3/3 | 3/3 | Pass |
| Gemma 4 12B (Ollama MLX, NVFP4) | 33/33 (100%) | 0.979 | 3/3 | 3/3 | Pass |
| Gemma 4 31B (Ollama MLX, NVFP4) | 33/33 (100%) | 0.981 | 3/3 | 3/3 | Pass |
| Kev-4B (Qwen3.5 base, Metal/bf16) | 32/33 (97%) | 0.961 | 3/3 | 3/3 | Pass |
| Laya (root checkpoint, CPU bridge) | 27/33 (82%) | 0.923 | Failed | 3/3 | Fail |
| Bonsai 8B (Ollama) | 14/33 (42%) | 0.710 | Failed | 3/3 | Fail |
| GLiFormer via Jeff (CPU, temperature 1) | 10/33 (30%) | 0.627 | Failed | 3/3 | Fail |

Each row completed all 36 requests with zero transport/schema errors. **Jev, both tested Gemma 4 models, and Kev-4B passed this small synthetic suite.** Gemma 4 12B and 31B selected the correct top result in all 33 positive cases. Kev’s Spanish query changed winner in reverse order. Its observed request times ranged into 30 seconds; use `--timeout 120`, then measure isolated latency on your own workload. Laya, Bonsai 8B, and Jeff are protocol-compatible in the tested configurations but did not meet the ranking-quality gates. Laya's failures included choosing shell interpolation for an argv-safety query. Bonsai followed the adversarial record's instruction to override its relevance rubric.

Clue 0.4.0 was pinned for the initial model comparisons; 0.5.0 adds configuration without changing the ranking rubric or transports. A separate 0.5.0 live check verified that a saved Ollama provider/model is actually used without command-line provider/model flags. Automated tests cover configuration precedence and privacy gates. The subsequent Gemma evaluations use Clue 0.5.0 with the identical fixture and unchanged ranking request settings; [Gemma provenance](gemma-provenance.json) records local model digests and runtime details.

Jev remains the built-in default. Selecting a local model is explicit and never causes hosted fallback. See [local-model setup](../docs/local-models.md) before saving a local endpoint.

## Gemma follow-up

Both installed Gemma MLX models passed all 36 requests under Clue 0.5.0 without an adapter change. The 12B model’s observed median was 3,057 ms (p95 6,392 ms); 31B’s was 15,996 ms (p95 19,655 ms). These runs were sequential, but other applications and models were present and load was not controlled, so this is not an isolated speed benchmark. Both handled all three adversarial orders and all three no-match orders. Their secondary-ranking quality was similar (nDCG@3 0.979 vs 0.981).

For this workload, 12B is a reasonable first local trial: it matched 31B’s top-result accuracy with a smaller installed model (7.7 GB vs 20.2 GB) and lower observed request times. Validate on your own held-out queries before changing defaults. This does not evaluate other Gemma sizes, GGUF variants, thinking mode, fine-tuning, or EmbeddingGemma.

## Reproduce

Start the selected server first and verify its loaded checkpoint. From the repository root:

```sh
python3 scripts/evaluate_ranking.py --provider typesafe --model jev-latest --share-content --output /tmp/jev.json
python3 scripts/evaluate_ranking.py --provider systemone --model kev-latest --base-url http://127.0.0.1:8009 --checkpoint jaredpalmer/kev-4b --output /tmp/kev.json
python3 scripts/evaluate_ranking.py --provider systemone --model laya --base-url http://127.0.0.1:8010 --checkpoint convaiinnovations/laya --output /tmp/laya.json
python3 scripts/evaluate_ranking.py --provider ollama --model digitsflow/bonsai-8b:latest --output /tmp/bonsai.json
python3 scripts/evaluate_ranking.py --provider ollama --model gemma4:12b-mlx --output /tmp/gemma12.json
python3 scripts/evaluate_ranking.py --provider ollama --model gemma4:31b-mlx --output /tmp/gemma31.json
```

Use `--clue /path/to/clue` to pin the executable and `--timeout` to set a request deadline. The runner preserves each completed request, marks early exits incomplete, and stops after three consecutive failures. Results are not used to tune prompts or labels and then reported as held-out accuracy.

## Runtime findings

- Jeff's default `JEFF_TEMPERATURE=3.2` returned a score inconsistent with the temperature-scaled distribution. Clue rejected 33 consecutive responses before that probe was stopped. Set `JEFF_TEMPERATURE=1` to satisfy the native weighted-score contract; the separate full evaluation records its resulting quality.
- Kev-4B initially ran on CPU because the sandbox hid Metal. That slow probe was interrupted; the completed evaluation uses verified `device: mps`, bf16, and the Qwen3.5-4B base. An early connection probe before that server was ready is not treated as a model-quality result.
- Laya root is English-oriented and the tested checkpoint is not its multilingual or typed-decisions fine-tune. The bridge's separate per-candidate formatting is part of the evaluated system.
- Bonsai 2 27B failed GGUF metadata loading in the installed runtime during the preceding live probe. No quality result is available for that model; Bonsai 8B is a different checkpoint.
- GLiFormer is evaluated through Jeff, not as a standalone extraction model. CUA-S1-FORMS has a different form-action task and is not evaluated as a relevance ranker.

- An additional Qwen3.8 27B MLX/Ollama probe stopped after 13 of 36 attempts, with six timeouts under a 20-second deadline while other GPU inference was active. Its partial score is not comparable with the completed runs, and it is not a standalone quality verdict.
