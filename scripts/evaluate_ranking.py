"""Evaluate Clue end to end on fixed synthetic labels; never passes labels to models."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time


def evaluate(args):
    fixture = Path(args.cases).read_bytes()
    suite = json.loads(fixture)
    records = []
    env = os.environ.copy()
    env["CLUE_CONFIG"] = args.config_path
    for key in ("CLUE_PROVIDER", "CLUE_MODEL", "CLUE_BASE_URL", "TYPESAFE_MODEL"):
        env.pop(key, None)
    # A private, absent config path isolates runs from personal defaults.
    command = [args.clue, "rank", "QUERY", "--provider", args.provider,
               "--model", args.model, "--timeout", str(args.timeout)]
    if args.base_url:
        command += ["--base-url", args.base_url]
    if args.share_content:
        command += ["--share-content"]
    failure_streak = 0
    for case in suite["cases"]:
        labeled = case["candidates"]
        grades = {r["id"]: r["grade"] for r in labeled}
        orders = [labeled, labeled[1:] + labeled[:1], list(reversed(labeled))]
        for permutation, rows in enumerate(orders):
            candidates = [{k: v for k, v in r.items() if k != "grade"} for r in rows]
            command[2] = case["query"]
            start = time.monotonic()
            try:
                proc = subprocess.run(command, input=json.dumps(candidates), text=True,
                                      capture_output=True, env=env, timeout=args.timeout + 10)
                data = json.loads(proc.stdout)
                if proc.returncode or not data.get("ok"):
                    raise ValueError(data.get("error", {}).get("message", "command failed"))
                results = data["results"]
                if len(results) != len(rows) or {r["id"] for r in results} != set(grades):
                    raise ValueError("source identity/count changed")
                dcg = lambda gs: sum((2**g-1)/math.log2(i+2) for i, g in enumerate(gs[:3]))
                ideal = dcg(sorted(grades.values(), reverse=True))
                scores = {r["id"]: r["relevance"]["score"] for r in results}
                record = {"case": case["id"], "order": permutation, "ok": True,
                          "input_ids": [r["id"] for r in rows],
                          "ranked_ids": [r["id"] for r in results], "scores": scores,
                          "top1": grades[results[0]["id"]] == max(grades.values()) if ideal else None,
                          "ndcg_at_3": dcg([grades[r["id"]] for r in results])/ideal if ideal else None,
                          "no_match_pass": max(scores.values()) <= suite["acceptance"]["no_match_max_score"] if not ideal else None,
                          "metadata": data.get("api")}
            except (ValueError, KeyError, subprocess.TimeoutExpired) as error:
                record = {"case": case["id"], "order": permutation, "ok": False,
                          "error": str(error) if not isinstance(error, subprocess.TimeoutExpired) else "evaluation timeout"}
            record["wall_ms"] = round((time.monotonic()-start)*1000)
            records.append(record)
            failure_streak = 0 if record["ok"] else failure_streak + 1
            Path(args.output).write_text(json.dumps({"incomplete": True, "records": records}, indent=2)+"\n")
            if failure_streak >= 3:
                break
        print(f"{case['id']}: {sum(r['ok'] for r in records[-3:])}/3 completed", flush=True)
        if failure_streak >= 3:
            break
    positive_ids = {c["id"] for c in suite["cases"] if any(r["grade"] for r in c["candidates"])}
    positive = [r for r in records if r["case"] in positive_ids]
    no_match = [r for r in records if r["case"] not in positive_ids]
    injection = [r for r in records if r["case"] == "injection"]
    summary = {
        "planned_requests":len(suite["cases"])*3, "requests": len(records), "errors": sum(not r["ok"] for r in records),
        "positive_top1": sum(r.get("top1", False) for r in positive)/len(positive),
        "mean_ndcg_at_3": sum(r.get("ndcg_at_3", 0) for r in positive)/len(positive),
        "no_match_passes": sum(r.get("no_match_pass", False) for r in no_match),
        "no_match_requests": len(no_match),
        "injection_top1": all(r.get("top1", False) for r in injection) if len(injection) == 3 else None,
        "median_wall_ms": statistics.median(r["wall_ms"] for r in records),
        "p95_wall_ms": sorted(r["wall_ms"] for r in records)[math.ceil(len(records)*.95)-1],
    }
    gates = suite["acceptance"]
    summary["passed_gates"] = (len(records) == len(suite["cases"])*3 and summary["errors"] <= gates["errors_max"]
        and summary["positive_top1"] >= gates["positive_top1_min"]
        and summary["mean_ndcg_at_3"] >= gates["mean_ndcg_at_3_min"]
        and summary["no_match_passes"] == len(no_match) and summary["injection_top1"])
    report = {"incomplete": len(records) != len(suite["cases"])*3, "label_provenance": suite["label_provenance"],
              "fixture_sha256": hashlib.sha256(fixture).hexdigest(),
              "clue_version": subprocess.check_output([args.clue, "--version"], text=True).strip(),
              "provider": args.provider, "model": args.model, "checkpoint": args.checkpoint,
              "acceptance": gates, "summary": summary, "records": records}
    Path(args.output).write_text(json.dumps(report, indent=2)+"\n")
    print(json.dumps(summary), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--clue", default="clue")
    parser.add_argument("--cases", default="evals/ranking-cases.json")
    parser.add_argument("--provider", required=True, choices=["typesafe", "systemone", "ollama"])
    parser.add_argument("--model", required=True)
    parser.add_argument("--base-url")
    parser.add_argument("--checkpoint", help="Actual checkpoint/revision loaded behind an API alias")
    parser.add_argument("--share-content", action="store_true")
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="clue-eval-") as directory:
        args.config_path = str(Path(directory) / "config.json")
        evaluate(args)
