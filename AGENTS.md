# cider-ai

Use `.agents/skills/typesafe-ai/SKILL.md` and current TypeSafe API docs for integration changes. This Rust project is independent of the Cider repository and Python experiment.

Keep stdout as versioned JSON except help/version and intentional skill-text export. Search is local by default. External ranking requires `--share-content`; shared credentials do not authorize sharing personal data. Real personal-name sharing was previously blocked by automatic approval review and has not been approved. Use synthetic content for API tests unless the user explicitly authorizes real content.

Only invoke fixed read-only Cider commands with argument arrays. Never run model output as commands. Do not log API keys, HTTP headers, or private search results. Keep private reports out of Git. Preserve distinct source failures, empty results, lexical scores, and model relevance judgments.

Run `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`. Keep dependencies locked. Workflow skills under `skills/` ship with this project; they are reference skills, not global agent policy.
