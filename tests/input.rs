use clue::input::{self, Format, Mapping};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn cli(args: &[&str], data: &str) -> (bool, Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_clue"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(data.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

#[test]
fn maps_github_records_without_losing_fields_or_sharing_unselected_fields() {
    let raw = json!({"number":7,"title":"Offline sync","body":"Keep notebook edits","url":"https://example.org/issue/7","labels":["bug"],"private_metadata":"not-for-model"});
    let (ok, out) = cli(
        &["collect", "--profile", "github-issues"],
        &json!([raw.clone()]).to_string(),
    );
    assert!(ok, "{out}");
    assert_eq!(out["results"][0]["id"], raw["url"]);
    assert_eq!(out["results"][0]["record"], raw);
    let items: Vec<clue::Candidate> = serde_json::from_value(out["results"].clone()).unwrap();
    let payload = clue::api::rank_payload("sync", &items, "jev-latest");
    assert!(!payload.to_string().contains("not-for-model"));
    assert!(!payload.to_string().contains("https://example.org"));
}
#[test]
fn jsonl_nested_paths_and_numeric_ids_work() {
    let data = "{\"issue\":{\"id\":42,\"name\":\"東京\"},\"body\":\"text\",\"extra\":1}\n\n{\"issue\":{\"id\":43,\"name\":\"Second\"},\"body\":null}\n";
    let (ok, out) = cli(
        &[
            "collect",
            "--id",
            "/issue/id",
            "--title",
            "/issue/name",
            "--text",
            "body",
        ],
        data,
    );
    assert!(ok, "{out}");
    assert_eq!(out["results"][0]["id"], "42");
    assert_eq!(out["results"][0]["record"]["extra"], 1);
    assert_eq!(out["results"][1]["text"], "");
}
#[test]
fn profiles_do_not_execute_implicitly_and_overrides_take_precedence() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("executed");
    let path = temp.path().join("profile.json");
    std::fs::write(&path,json!({"name":"test","description":"fixture","argv":["touch",marker],"mapping":{"id":"wrong","title":"title"}}).to_string()).unwrap();
    let (ok, out) = cli(
        &[
            "collect",
            "--profile-file",
            path.to_str().unwrap(),
            "--id",
            "number",
        ],
        r#"[{"number":1,"title":"Keep me","custom":true}]"#,
    );
    assert!(ok, "{out}");
    assert!(!marker.exists());
    assert_eq!(out["results"][0]["id"], "1");
}
#[test]
fn malformed_duplicate_and_oversize_inputs_fail_without_silent_truncation() {
    for data in [
        r#"[{"id":"a","title":"one"},{"id":"a","title":"two"}]"#,
        r#"{"schema_version":1,"ok":false,"results":[]}"#,
        r#"{"title":"missing id"}"#,
        "{bad json}",
    ] {
        assert!(!cli(&["collect"], data).0, "accepted {data}");
    }
    let rows = (0..51)
        .map(|i| json!({"id":i,"title":"item"}))
        .collect::<Vec<_>>();
    assert!(!cli(&["collect"], &json!(rows).to_string()).0);
    assert!(input::parse(&vec![b' '; 4_194_305], Format::Auto).is_err());
    assert!(input::parse(b"{\"id\":1}\n{\"id\":2}", Format::Json).is_err());
    let (ok, out) = cli(&["collect"], "[]");
    assert!(ok);
    assert_eq!(out["results"], json!([]));
}
#[test]
fn preserves_partial_upstream_status_and_measures_bundle_with_original_records() {
    let data = json!({"schema_version":1,"ok":true,"partial":true,"sources":[{"ok":false,"source":"notes"}],"results":[{"id":"1","title":"short"},{"id":"2","title":"あ".repeat(300)}]});
    let (ok, out) = cli(&["bundle", "--budget-bytes", "400"], &data.to_string());
    assert!(ok, "{out}");
    assert_eq!(out["partial"], true);
    assert_eq!(out["upstream"]["sources"][0]["source"], "notes");
    assert_eq!(out["context"]["omitted"], 1);
    assert_eq!(
        out["context"]["context_bytes"],
        serde_json::to_vec(&out["results"]).unwrap().len()
    );
}
#[test]
fn all_builtin_profiles_parse_and_legacy_candidates_still_work() {
    for (name, _) in input::PROFILES {
        assert!(!input::profile(name).unwrap().argv.is_empty());
    }
    let rows =
        vec![json!({"id":"r","title":"old format","source":"reminders","lexical_score":2.0})];
    assert_eq!(
        input::normalize(rows, &Mapping::default(), false).unwrap()[0].lexical_score,
        2.0
    );
}

#[cfg(unix)]
mod commands {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn executable(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("fake-cli");
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    #[test]
    fn command_argv_is_literal_and_child_does_not_receive_typesafe_secrets() {
        let temp = tempfile::tempdir().unwrap();
        let path = executable(
            temp.path(),
            "#!/bin/sh\n[ -z \"$TYPESAFE_API_KEY$TYPESAFE_API_KEY_FILE$CLUE_PROVIDER_API_KEY\" ] || exit 9\n[ \"$1\" = '$(touch NEVER)' ] || exit 8\nprintf '%s' '[{\"id\":\"a\",\"title\":\"Works\"}]'\n",
        );
        let out = Command::new(env!("CARGO_BIN_EXE_clue"))
            .args(["collect", "--", path.to_str().unwrap(), "$(touch NEVER)"])
            .env("TYPESAFE_API_KEY", "fixture-secret")
            .env("TYPESAFE_API_KEY_FILE", "fixture-path")
            .env("CLUE_PROVIDER_API_KEY", "provider-secret")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
    #[test]
    fn rank_checks_sharing_before_launching_a_command() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("should-not-exist");
        let out = Command::new(env!("CARGO_BIN_EXE_clue"))
            .args(["rank", "test", "--", "touch", marker.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(!marker.exists());
        assert!(String::from_utf8_lossy(&out.stdout).contains("--share-content"));
    }
    #[tokio::test]
    async fn source_failure_and_timeout_do_not_look_like_empty_results() {
        let temp = tempfile::tempdir().unwrap();
        let path = executable(
            temp.path(),
            "#!/bin/sh\nprintf '%s' '[{\"id\":\"a\",\"title\":\"ignore me\"}]'\nexit 2\n",
        );
        assert!(
            input::execute(
                &[path.to_string_lossy().into()],
                std::time::Duration::from_secs(1)
            )
            .await
            .is_err()
        );
        let path = executable(temp.path(), "#!/bin/sh\nexec sleep 5\n");
        let start = std::time::Instant::now();
        assert!(
            input::execute(
                &[path.to_string_lossy().into()],
                std::time::Duration::from_millis(30)
            )
            .await
            .is_err()
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
    #[tokio::test]
    async fn excessive_child_output_is_stopped_without_waiting_for_exit() {
        let result = input::execute(
            &["yes".into(), "synthetic-output".into()],
            std::time::Duration::from_secs(2),
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("4 MiB"));
    }
    #[test]
    fn explicit_profile_execution_and_command_input_conflicts() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("executed");
        let path = executable(
            temp.path(),
            "#!/bin/sh\nprintf '%s' '[{\"number\":42,\"title\":\"Mapped\"}]'\n",
        );
        let profile = temp.path().join("profile.json");
        std::fs::write(&profile,json!({"name":"fixture","description":"test","argv":[path],"mapping":{"id":"number","title":"title"}}).to_string()).unwrap();
        let (ok, out) = cli(
            &[
                "collect",
                "--profile-file",
                profile.to_str().unwrap(),
                "--run-profile",
            ],
            "",
        );
        assert!(ok, "{out}");
        assert_eq!(out["results"][0]["id"], "42");
        let (ok, _) = cli(
            &[
                "collect",
                "--input",
                "unused",
                "--",
                "touch",
                marker.to_str().unwrap(),
            ],
            "",
        );
        assert!(!ok);
        assert!(!marker.exists());
    }
}
