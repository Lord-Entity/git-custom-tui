use crate::model::Config;
use anyhow::{Context, Result};
use std::{fs, path::PathBuf};

fn path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not locate the home directory")?;
    Ok(home
        .join(".config")
        .join("git-commit-tui")
        .join("config.json"))
}

pub fn load() -> Result<Config> {
    let file = path()?;
    if !file.exists() {
        return Ok(Config::default());
    }
    let data =
        fs::read_to_string(&file).with_context(|| format!("could not read {}", file.display()))?;
    serde_json::from_str(&data).with_context(|| format!("invalid config file {}", file.display()))
}

pub fn save(config: &Config) -> Result<()> {
    let file = path()?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    fs::write(
        &file,
        format!("{}\n", serde_json::to_string_pretty(config)?),
    )
    .with_context(|| format!("could not write {}", file.display()))
}

pub fn clear() -> Result<()> {
    let file = path()?;
    if file.exists() {
        fs::remove_file(&file).with_context(|| format!("could not remove {}", file.display()))?;
    }
    Ok(())
}
