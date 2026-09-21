use crate::provider::{Options, Provider};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub provider: Provider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

pub fn path() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("CLUE_CONFIG") {
        let p = PathBuf::from(value);
        ensure!(p.is_absolute(), "CLUE_CONFIG must be an absolute file path");
        return Ok(p);
    }
    let base = if let Some(value) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(value);
        ensure!(p.is_absolute(), "XDG_CONFIG_HOME must be absolute");
        p
    } else {
        PathBuf::from(std::env::var_os("HOME").context("HOME is unset")?).join(".config")
    };
    Ok(base.join("clue/config.json"))
}

pub fn read(path: &Path) -> Result<Option<Saved>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => anyhow::bail!("cannot read Clue config file"),
    };
    ensure!(
        file.metadata()?.is_file(),
        "Clue config must be a regular file"
    );
    let mut data = Vec::new();
    file.take(16385).read_to_end(&mut data)?;
    ensure!(data.len() <= 16384, "Clue config exceeds 16 KiB");
    let saved: Saved = serde_json::from_slice(&data)
        .map_err(|_| anyhow::anyhow!("invalid Clue config: expected provider, optional model/base_url; secrets and sharing consent do not belong in config"))?;
    validate(&saved)?;
    Ok(Some(saved))
}

pub fn load() -> Result<Option<Saved>> {
    read(&path()?)
}

fn validate(saved: &Saved) -> Result<()> {
    Options {
        provider: saved.provider,
        model: saved.model.clone(),
        base_url: saved.base_url.clone(),
    }
    .resolve(true)?; // URL/schema validation only. Saving defaults never grants sharing consent.
    Ok(())
}

pub fn save(path: &Path, saved: &Saved) -> Result<()> {
    validate(saved)?;
    ensure!(
        !path.is_symlink(),
        "refusing to replace a symlink config file"
    );
    let parent = path.parent().context("config path has no parent")?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".clue-config-{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let result = (|| -> Result<()> {
        let mut file = options.open(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(saved)?)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    let _ = fs::remove_file(tmp);
    result
}

pub fn reset() -> Result<()> {
    let path = path()?;
    ensure!(
        !path.is_symlink(),
        "refusing to remove a symlink config file"
    );
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
