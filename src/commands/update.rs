use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::cli::UpdateArgs;
use crate::config::load_config;
use crate::release::{latest_release, normalize_github_repo};

pub fn run(args: UpdateArgs) -> Result<()> {
    let loaded = load_config(args.config_path)?;
    let repo = loaded
        .config
        .release
        .github_repo
        .clone()
        .ok_or_else(|| anyhow::anyhow!("github_repo is not configured in lilaccaps.toml"))?;
    let normalized_repo = normalize_github_repo(&repo)?;
    let release = latest_release(Some(&normalized_repo))?
        .ok_or_else(|| anyhow::anyhow!("no stable release found for {normalized_repo}"))?;

    let status = Command::new("cargo")
        .arg("install")
        .arg("--git")
        .arg(format!("https://github.com/{normalized_repo}.git"))
        .arg("--tag")
        .arg(&release.tag_name)
        .arg("lilaccaps")
        .status()
        .with_context(|| "failed to start cargo install for lilaccaps update")?;

    if !status.success() {
        bail!("cargo install failed while updating lilaccaps");
    }

    println!("updated = true");
    println!("repo = {}", normalized_repo);
    println!("version = {}", release.version);
    println!("tag = {}", release.tag_name);

    Ok(())
}
