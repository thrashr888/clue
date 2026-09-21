use clue::{
    config::{self, Saved},
    provider::Provider,
};
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn command(path: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_clue"));
    cmd.env("CLUE_CONFIG", path);
    for key in [
        "CLUE_PROVIDER",
        "CLUE_MODEL",
        "CLUE_BASE_URL",
        "TYPESAFE_MODEL",
    ] {
        cmd.env_remove(key);
    }
    cmd
}

#[test]
fn persisted_defaults_and_overrides_are_provider_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clue/config.json");
    let out = command(&path)
        .args([
            "config",
            "set",
            "--provider",
            "ollama",
            "--model",
            "local-one",
            "--base-url",
            "http://127.0.0.1:11434",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        config::read(&path).unwrap().unwrap().model.as_deref(),
        Some("local-one")
    );
    let out = command(&path).args(["config", "show"]).output().unwrap();
    let d: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(d["effective"]["provider"], "ollama");
    assert_eq!(d["effective"]["model"], "local-one");
    let out = command(&path)
        .env("CLUE_MODEL", "env-model")
        .args(["config", "show", "--model", "flag-model"])
        .output()
        .unwrap();
    let d: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(d["effective"]["model"], "flag-model");
    let out = command(&path)
        .env("CLUE_MODEL", "env-model")
        .args(["config", "show"])
        .output()
        .unwrap();
    let d: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(d["effective"]["model"], "env-model");
    let out = command(&path)
        .args(["config", "show", "--provider", "typesafe"])
        .output()
        .unwrap();
    let d: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(d["effective"]["model"], "jev-latest");
    assert_eq!(d["effective"]["base_url"], "https://api.typesafe.ai/");
    assert_eq!(d["sharing_authorized"], false);
    assert!(
        command(&path)
            .args(["config", "reset"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!path.exists());
}

#[test]
fn invalid_defaults_fail_before_source_execution_and_never_store_consent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let marker = dir.path().join("ran");
    for content in [
        "invalid JSON".to_string(),
        json!({"provider":"typesafe","share_content":true}).to_string(),
        json!({"provider":"ollama","model":"foo","api_key":"secret"}).to_string(),
    ] {
        std::fs::write(&path, content).unwrap();
        let out = command(&path)
            .args(["rank", "test", "--", "touch", marker.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(!marker.exists());
    }
    config::save(
        &path,
        &Saved {
            provider: Provider::Systemone,
            model: Some("remote".into()),
            base_url: Some("https://models.example.org".into()),
        },
    )
    .unwrap();
    let out = command(&path)
        .args(["rank", "test", "--", "touch", marker.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("--share-content"));
    assert!(!marker.exists());
}

#[test]
fn invalid_save_preserves_config_and_reads_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let valid = Saved {
        provider: Provider::Ollama,
        model: Some("local".into()),
        base_url: None,
    };
    config::save(&path, &valid).unwrap();
    let invalid = Saved {
        provider: Provider::Ollama,
        model: None,
        base_url: None,
    };
    assert!(config::save(&path, &invalid).is_err());
    assert_eq!(config::read(&path).unwrap(), Some(valid));
    std::fs::write(&path, " ".repeat(16385)).unwrap();
    assert!(
        config::read(&path)
            .unwrap_err()
            .to_string()
            .contains("16 KiB")
    );
}

#[cfg(unix)]
#[test]
fn save_does_not_replace_symlink_targets() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let path = dir.path().join("config.json");
    std::fs::write(&target, "untouched").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(
        config::save(
            &path,
            &Saved {
                provider: Provider::Typesafe,
                model: Some("jev-latest".into()),
                base_url: None
            }
        )
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(target).unwrap(), "untouched");
}

#[test]
fn xdg_global_config_path_is_used() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_clue"))
        .env_remove("CLUE_CONFIG")
        .env("XDG_CONFIG_HOME", dir.path())
        .args([
            "config",
            "set",
            "--provider",
            "ollama",
            "--model",
            "example",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(dir.path().join("clue/config.json").is_file());
}
