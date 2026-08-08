use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::cli::UpdateArgs;
use crate::config::load_or_init_config;
use crate::release::{default_github_repo, latest_release, normalize_github_repo};
use crate::runtime::{
    CARGO_DEPENDENCY, CMAKE_DEPENDENCY, DependencyUpdateReport, cargo_install_root,
    ensure_dependency, install_binary_path, update_dependencies_with_brew,
};

pub fn run(args: UpdateArgs) -> Result<()> {
    ensure_dependency(CARGO_DEPENDENCY)?;
    let config_path = args.config_path.clone();
    let loaded = load_or_init_config(config_path.clone())?;
    let repo = loaded
        .config
        .release
        .github_repo
        .clone()
        .unwrap_or_else(default_github_repo);
    let normalized_repo = normalize_github_repo(&repo)?;
    let release = latest_release(Some(&normalized_repo))?
        .ok_or_else(|| anyhow::anyhow!("no stable release found for {normalized_repo}"))?;

    let dependency_update = if args.skip_dependencies {
        DependencyUpdateReport {
            updated_packages: Vec::new(),
            skipped_reason: Some("dependency updates were skipped by request".to_string()),
        }
    } else {
        update_dependencies_with_brew()?
    };
    ensure_dependency(CMAKE_DEPENDENCY)?;

    let install_root = cargo_install_root()?;
    let status = Command::new("cargo")
        .arg("install")
        .arg("--root")
        .arg(&install_root)
        .arg("--git")
        .arg(format!("https://github.com/{normalized_repo}.git"))
        .arg("--tag")
        .arg(&release.tag_name)
        .arg("--locked")
        .arg("--force")
        .arg("lilaccaps")
        .status()
        .with_context(|| "failed to start cargo install for lilaccaps update")?;

    if !status.success() {
        bail!("cargo install failed while updating lilaccaps");
    }

    let installed_binary = install_binary_path()?;
    let mut refresh = Command::new(&installed_binary);
    refresh.arg("install");
    if let Some(config_path) = config_path {
        refresh.arg("--config-path").arg(config_path);
    }
    let refresh_status = refresh.status().with_context(|| {
        format!(
            "failed to start the updated lilaccaps binary at {}",
            installed_binary.display()
        )
    })?;
    if !refresh_status.success() {
        bail!("updated lilaccaps installed but post-update setup validation failed");
    }

    println!("updated = true");
    println!("repo = {}", normalized_repo);
    println!("version = {}", release.version);
    println!("tag = {}", release.tag_name);
    println!("binary_path = {}", installed_binary.display());
    println!(
        "dependency_update = {}",
        if dependency_update.skipped_reason.is_some() {
            "skipped"
        } else {
            "completed"
        }
    );
    println!(
        "dependency_packages = {}",
        if dependency_update.updated_packages.is_empty() {
            "none".to_string()
        } else {
            dependency_update.updated_packages.join(", ")
        }
    );
    println!(
        "dependency_update_reason = {}",
        dependency_update
            .skipped_reason
            .as_deref()
            .unwrap_or("none")
    );

    Ok(())
}
