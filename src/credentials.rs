use anyhow::{Context, Result, bail, ensure};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub struct Credential {
    pub key: String,
    pub source: String,
}

pub fn shared_path() -> Result<PathBuf> {
    if let Some(base) = env::var_os("XDG_CONFIG_HOME") {
        let base = PathBuf::from(base);
        ensure!(base.is_absolute(), "XDG_CONFIG_HOME must be absolute");
        return Ok(base.join("typesafe/api-key"));
    }
    Ok(PathBuf::from(env::var_os("HOME").context("HOME is unset")?)
        .join(".config/typesafe/api-key"))
}

fn clean(value: String) -> Result<String> {
    let value = value.trim();
    ensure!(
        !value.is_empty() && value.len() <= 8192 && !value.chars().any(char::is_whitespace),
        "API key must be one nonempty token of at most 8192 bytes"
    );
    Ok(value.into())
}

pub fn read_key_file(path: &Path) -> Result<String> {
    let metadata = fs::metadata(path).context("cannot read credential file metadata")?;
    ensure!(
        metadata.is_file() && metadata.len() <= 8192,
        "credential file must be a regular file of at most 8192 bytes"
    );
    #[cfg(unix)]
    ensure!(
        metadata.permissions().mode() & 0o077 == 0,
        "credential file is accessible to other users; run chmod 600 on it"
    );
    clean(fs::read_to_string(path).context("cannot read credential file")?)
}

pub fn load() -> Result<Credential> {
    if let Ok(value) = env::var("TYPESAFE_API_KEY") {
        return Ok(Credential {
            key: clean(value)?,
            source: "environment:TYPESAFE_API_KEY".into(),
        });
    }
    let path = env::var_os("TYPESAFE_API_KEY_FILE")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(shared_path)?;
    if !path.exists() {
        bail!(
            "no TypeSafe credential; set TYPESAFE_API_KEY, TYPESAFE_API_KEY_FILE, or run cider-ai auth set"
        );
    }
    Ok(Credential {
        key: read_key_file(&path)?,
        source: format!("file:{}", path.display()),
    })
}

pub fn save(path: &Path, key: String, force: bool) -> Result<()> {
    let key = clean(key)?;
    let parent = path.parent().context("credential path has no parent")?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let temporary = parent.join(format!(".api-key-{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let result = (|| -> Result<()> {
        let mut file = options
            .open(&temporary)
            .context("cannot create private credential file")?;
        file.write_all(key.as_bytes())?;
        file.sync_all()?;
        if force {
            ensure!(
                !path.is_symlink(),
                "refusing to replace a symlink credential file"
            );
            fs::rename(&temporary, path)?;
        } else {
            // Linking publishes atomically without replacing an existing credential.
            fs::hard_link(&temporary, path).context(
                "credential already exists or cannot be published; use --force to replace",
            )?;
            fs::remove_file(&temporary)?;
        }
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result
}

pub fn from_stdin() -> Result<String> {
    let mut value = String::new();
    std::io::stdin().take(8193).read_to_string(&mut value)?;
    clean(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_atomic_storage_does_not_overwrite_without_force() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("typesafe/api-key");
        save(&path, "test-secret".into(), false).unwrap();
        assert_eq!(read_key_file(&path).unwrap(), "test-secret");
        assert!(save(&path, "other".into(), false).is_err());
        assert_eq!(read_key_file(&path).unwrap(), "test-secret");
        save(&path, "new".into(), true).unwrap();
        assert_eq!(read_key_file(&path).unwrap(), "new");
        #[cfg(unix)]
        {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(read_key_file(&path).is_err());
        }
    }
}
