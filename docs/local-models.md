# Local model providers

Reviewed against upstream documentation on 2026-09-21. Clue keeps retrieval and execution in code; models only judge supplied candidates.

| Model/runtime | Fit for Clue | Integration |
| --- | --- | --- |
| [Kev](https://github.com/jaredpalmer/kev) | Closest native decision API fit. Supports Choice, Noul, Score with distributions; Apple Silicon and CUDA. | `--provider systemone`, port 8009, model `kev-latest`. |
| [Kev-0.5B](https://huggingface.co/jaredpalmer/kev-0.5b) | Historical Qwen2.5 prototype. Upstream now recommends the 0.8B/4B/9B family, with earlier Qwen3 models still available for Mac latency. | Use current server/model instructions, rather than assuming the prototype is the best checkpoint. |
| [Laya](https://huggingface.co/convaiinnovations/laya) | Small encoder decision model with English, multilingual, and specialized checkpoints. Root model: 421M parameters, 512-token context. Its own card reports weak zero-shot ordinal scoring and overconfidence. | Optional Python bridge in `scripts/serve_laya.py`; not an Ollama chat model. Evaluate on your own relevance labels. |
| [GLiFormer](https://huggingface.co/knowledgator/gliformer-large-v1) / [Jeff](https://github.com/logan-markewich/jeff) | GLiFormer is a 575.6M encoder for classification/extraction. Jeff exposes a System One API around it. Extraction may be as useful as reranking for future Clue workflows. | Generic System One provider. Jeff documentation requires a server bearer key. See the [end-to-end evaluation](../evals/README.md) for live results and required server settings. |
| [Gemma 4](https://ai.google.dev/gemma/docs/core) | General-purpose model family; installed 12B and 31B MLX variants are evaluated with Clue’s existing structured-rating rubric. | Ollama provider; see the Gemma section and evaluation results below. |
| [Bonsai 2 27B](https://prismml.com/news/bonsai-2-27b) | A compressed generative model; potentially useful for scoring through structured output. Its published memory/speed claims are not Clue measurements. | Ollama provider if the installed runtime supports its weight format. The installed GGUF failed metadata loading on this machine; see validation below. |
| [CUA-S1-FORMS Core ML](https://huggingface.co/FluidInference/cua-s1-forms-coreml) | A 706K-parameter specialist choosing supplied form actions. Relevant to testing form workflows in Tauri Browser, rather than general text relevance. | No Clue ranking adapter: a different task/schema and Core ML runtime. |

These model families do not all run in Ollama. Kev, Laya, and GLiFormer use custom decision heads; preserving their native inference requires their runtimes. Ollama uses the documented [chat API](https://docs.ollama.com/api/chat) with [structured outputs](https://docs.ollama.com/capabilities/structured-outputs). Clue validates generated ratings and never labels them as calibrated distributions.

## Kev

Follow upstream setup in a separate directory:

```sh
git clone https://github.com/jaredpalmer/kev
cd kev
uv sync --extra serve
KEV_DTYPE=bf16 uv run --extra serve python -m kev.serve --run jaredpalmer/kev-4b --port 8009
```

Then, from Clue:

```sh
clue rank "offline notebook sync" --input examples/candidates.json \
  --provider systemone --base-url http://127.0.0.1:8009 --model kev-latest --timeout 120
```

The model field is the API alias; Kev's server startup chooses the actual checkpoint. Record that checkpoint alongside comparisons. Upstream reports faster Apple Silicon inference for the previous Qwen3 family (`jaredpalmer/kev-0.6b`, `jaredpalmer/kev-4b@qwen3`, `jaredpalmer/kev-8b`) than its Qwen3.5 family at the review date.

## Laya

From a Clue checkout (the bridge is optional and is not bundled into the Rust executable):

```sh
uv run scripts/serve_laya.py --device cpu
# Optional checkpoint trained for specific typed-decision workflows:
uv run scripts/serve_laya.py --subfolder typed-decisions --port 8011
```

The first run downloads dependencies and the selected checkpoint. Default binding is always `127.0.0.1`; there is no remote bind flag. The bridge uses Laya 0.3.4 and Transformers 4.x. It disables TensorFlow import probing to avoid an upstream-documented hang. CPU is the tested default; other devices depend on the local PyTorch build.

```sh
clue rank "offline notebook sync" --input examples/candidates.json \
  --provider systemone --base-url http://127.0.0.1:8010 --model laya --timeout 120
```

This is specifically a Clue ranking bridge, not a complete System One implementation. It isolates each candidate, applies the same four-level relevance rubric, and rejects evidence that exceeds the checkpoint's context budget instead of silently truncating it. Clue already caps title/text at 300/1,600 characters; these character bounds do not guarantee Laya token fit. For HTTP 422, use shorter input or a larger-context checkpoint. The bridge does not claim its different prompt formatting is semantically identical to Jev's.

## Jeff

Follow [Jeff's setup](https://github.com/logan-markewich/jeff), set `JEFF_TEMPERATURE=1` so scores match their probability distributions, select a server key, and supply that same key through `CLUE_PROVIDER_API_KEY` (never command-line arguments). Use:

```sh
clue rank "offline notebook sync" --input examples/candidates.json \
  --provider systemone --base-url http://127.0.0.1:8000 --model jev-latest --timeout 120
```

The API model alias here is Jeff's documented accepted value, not a request to TypeSafe. Clue does not forward the shared TypeSafe credential.

## Gemma

Gemma 4 uses the existing Ollama provider; no dedicated adapter is needed. The evaluated local tags are `gemma4:12b-mlx` and `gemma4:31b-mlx`, running through Ollama 0.34.2. Both passed the fixed 36-request suite with 33/33 correct positive top results, all adversarial and no-match checks, and no response errors. 12B is a reasonable first local trial; its observed median was about 3 seconds versus 16 seconds for 31B, though system load was not controlled. Exact installed digests, quantization, and request options are in [gemma-provenance.json](../evals/gemma-provenance.json); results use the same fixed labels and rubric as the other models.

```sh
clue rank "offline notebook sync" --input examples/candidates.json \
  --provider ollama --model gemma4:12b-mlx --timeout 120
# Once you have evaluated the installed model, optionally save it:
clue config set --provider ollama --model gemma4:12b-mlx
```

Clue requests temperature 0, thinking disabled, and JSON-schema ratings. These are generated integer ratings, without native probability distributions. This evaluates the shipped Clue configuration rather than tuning Gemma settings on the test cases. See the [full results](../evals/README.md) before choosing a default.

[EmbeddingGemma](https://ai.google.dev/gemma/docs/embeddinggemma) is a separate 308M text-embedding model that could retrieve semantic candidates before reranking. It is not a drop-in ranking provider, and this evaluation does not test an embedding index.

## Validation

The [end-to-end evaluation](../evals/README.md) supersedes the initial smoke conclusions below. Across 36 requests per model (12 synthetic scenarios in three orders), Jev and both tested Gemma 4 models achieved 33/33 correct top results; Kev-4B achieved 32/33. All four passed the predeclared gates, including adversarial and no-match cases. Laya achieved 27/33, Bonsai 8B 14/33, and Jeff with temperature 1 achieved 10/33; all three failed the adversarial gate. These are small assistant-labeled fixtures, not production validation or independent human judgments.

After starting the Kev server above, save it as your default:

```sh
clue config set --provider systemone --model kev-latest --base-url http://127.0.0.1:8009
clue config show
clue rank "offline notebook sync" --input examples/candidates.json --timeout 120
# Restore hosted defaults (individual ranking calls still require --share-content):
clue config set --provider typesafe --model jev-latest
```

Configuration does not launch the model server. Kev-4B took over 30 seconds for some requests in this evaluation; wall times were collected during concurrent inference and are not controlled latency comparisons.

### Earlier connectivity probes


All four providers ranked the notebook-sync fixture first. Jev scored it 2.97/3 (291 ms), Laya 2.29/3 (448 ms, CPU), and Ollama Bonsai 8B 3/3 (1,644 ms). Kev-0.6B took 10,298 ms on its first ranking request and returned scores from 2.53 to 2.60 for all four records, including unrelated groceries and wallpaper: a connectivity success with poor relevance separation. This run does not evaluate the current Kev-4B/9B checkpoints. Laya also gave the canvas-layout record 1.25, while Bonsai 8B gave it 2; neither smoke result establishes production relevance quality.

See [provider-validation.json](provider-validation.json) for the recorded synthetic smoke checks. The inputs are public repository fixtures; no personal notebook or Calendar contents were sent to a hosted model. These small hand-selected examples check connectivity and obvious ordering, not calibrated accuracy, workload performance, or superiority over another model.

For a useful evaluation next, label representative retrieved candidates (including absent answers, weak keyword overlap, long snippets, multilingual content, and adversarial instructions), hold out queries, and compare nDCG/Recall@k, latency, abstention, context rejection, and failure rate. Keep native distributions separate from generated integer ratings.
