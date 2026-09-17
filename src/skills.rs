use anyhow::{Result, ensure};
use std::{fs, path::Path};
pub const SKILLS: &[(&str, &str)] = &[
    (
        "cider-ai-search",
        include_str!("../skills/cider-ai-search/SKILL.md"),
    ),
    (
        "cider-ai-context",
        include_str!("../skills/cider-ai-context/SKILL.md"),
    ),
    (
        "cider-ai-rank",
        include_str!("../skills/cider-ai-rank/SKILL.md"),
    ),
];

pub fn install(dir: &Path, force: bool) -> Result<Vec<String>> {
    for (name, body) in SKILLS {
        let path = dir.join(name).join("SKILL.md");
        if path.exists() && !force {
            ensure!(
                fs::read_to_string(&path)? == *body,
                "skill {name} already exists with different content; use another directory or --force"
            );
        }
    }
    let mut paths = Vec::new();
    for (name, body) in SKILLS {
        let folder = dir.join(name);
        fs::create_dir_all(&folder)?;
        let path = folder.join("SKILL.md");
        fs::write(&path, body)?;
        paths.push(path.display().to_string());
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installation_preserves_edited_skills_unless_requested() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(install(temp.path(), false).unwrap().len(), 3);
        let path = temp.path().join("cider-ai-search/SKILL.md");
        fs::write(&path, "custom").unwrap();
        assert!(install(temp.path(), false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "custom");
        assert!(install(temp.path(), true).is_ok());
    }
}
