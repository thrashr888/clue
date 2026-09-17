#![cfg(unix)]
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

fn fixture() -> (tempfile::TempDir, String) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cider");
    fs::write(&path,r##"#!/bin/sh
if [ -n "$TYPESAFE_API_KEY" ]; then exit 9; fi
case "$1" in
reminders) printf '%s\n' '[{"id":"a","title":"Build notebook sharing","list":"Atlas","notes":"iCloud sync decisions","completed":false},{"id":"b","title":"Finished sync work","list":"Atlas","notes":"done","completed":true}]' ;;
notes) printf '%s\n' '[{"id":"n","title":"Notebook sync RFC","folder":"Research"}]' ;;
calendar) printf '%s\n' '[{"id":"event-1","title":"Notebook sync planning","calendar":"Work","start_date":"2026-09-18T10:00:00","end_date":"2026-09-18T11:00:00","is_all_day":false}]' ;;
safari) exit 2 ;;
*) printf '%s\n' '{}' ;;
esac
"##).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    (temp, path.display().to_string())
}

#[test]
fn partial_failures_preserve_results_and_source_ids() {
    let (_temp, path) = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_cider-ai"))
        .args([
            "--cider",
            &path,
            "search",
            "notebook sync",
            "--sources",
            "reminders,notes,safari",
            "--list",
            "Atlas",
        ])
        .env("TYPESAFE_API_KEY", "fixture-secret")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let data: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(data["schema_version"], 1);
    assert_eq!(data["partial"], true);
    assert_eq!(data["results"].as_array().unwrap().len(), 2);
    assert!(
        data["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "reminders:a")
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-secret"));
}

#[test]
fn all_failed_is_error_not_empty_success() {
    let (_temp, path) = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_cider-ai"))
        .args(["--cider", &path, "search", "test", "--sources", "safari"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let data: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(data["error"]["code"], "all_sources_failed");
}

#[test]
fn context_budget_matches_actual_serialized_records() {
    let (_temp, path) = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_cider-ai"))
        .args([
            "--cider",
            &path,
            "context",
            "sync",
            "--sources",
            "reminders,notes",
            "--budget-bytes",
            "300",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let data: Value = serde_json::from_slice(&output.stdout).unwrap();
    let bytes = serde_json::to_vec(&data["results"]).unwrap().len();
    assert!(bytes <= 300);
    assert_eq!(data["context"]["context_bytes"], bytes);
}

#[test]
fn rank_never_reads_stdin_or_network_without_sharing_opt_in() {
    let output = Command::new(env!("CARGO_BIN_EXE_cider-ai"))
        .args(["rank", "test"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let data: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        data["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--share-content")
    );
}

#[test]
fn credential_environment_precedes_file_without_leaking_key() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_cider-ai"))
        .args(["auth", "status"])
        .env("TYPESAFE_API_KEY", "environment-placeholder")
        .env("TYPESAFE_API_KEY_FILE", temp.path().join("missing"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let data: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(data["credential"]["source"], "environment:TYPESAFE_API_KEY");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("environment-placeholder"));
}

#[test]
fn calendar_search_exposes_event_metadata() {
    let (_temp, path) = fixture();
    let out = Command::new(env!("CARGO_BIN_EXE_cider-ai"))
        .args([
            "--cider",
            &path,
            "search",
            "sync",
            "--sources",
            "calendar",
            "--calendar",
            "Work",
            "--days-ahead",
            "14",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let data: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(data["results"][0]["event"]["calendar"], "Work");
    assert_eq!(data["results"][0]["event"]["is_all_day"], false);
}

#[test]
fn sqlite_cli_queries_are_rank_compatible_and_sharing_is_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("data.db");
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(include_str!("../examples/projects.sql"))
        .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_cider-ai"))
        .args([
            "sqlite",
            "query",
            path.to_str().unwrap(),
            "SELECT id,title,description AS text FROM issues ORDER BY id",
            "--candidates",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let data: Value = serde_json::from_slice(&out.stdout).unwrap();
    let items: Vec<cider_ai::Candidate> = serde_json::from_value(data["results"].clone()).unwrap();
    cider_ai::api::validate_candidates(&items).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_cider-ai"))
        .args([
            "sqlite",
            "search",
            path.to_str().unwrap(),
            "sync",
            "--table",
            "issues",
            "--columns",
            "description",
            "--ai",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
