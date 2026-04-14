use std::env;
use std::fs;

use anyhow::{Context, Result};

use crate::cli::InstallArgs;
use crate::config::{LoadedConfig, load_or_init_config};
use crate::integration::{ensure_skill_file, write_bootstrap_markdown};
use crate::media::ensure_ffmpeg_available;
use crate::model::ensure_model_downloaded;
use crate::runtime::{
    cargo_bin_dir, current_executable, ensure_dir, install_binary_path, models_dir, tmp_dir,
};

pub fn run(args: InstallArgs) -> Result<()> {
    let LoadedConfig {
        paths,
        config,
        created,
    } = load_or_init_config(args.config_path)?;

    ensure_dir(&paths.runtime_home)?;
    ensure_dir(&models_dir(&paths.runtime_home))?;
    ensure_dir(&tmp_dir(&paths.runtime_home))?;
    ensure_dir(&cargo_bin_dir()?)?;
    ensure_ffmpeg_available()?;

    let bootstrap_path = write_bootstrap_markdown(&paths, &config)?;
    let binary_path = install_binary()?;
    let model_path = ensure_model_downloaded(&paths, &config)?;
    let skill_path = ensure_skill_file(&config)?;

    println!("installed = true");
    println!("binary_path = {}", binary_path.display());
    println!("config_path = {}", paths.config_path.display());
    println!("runtime_home = {}", paths.runtime_home.display());
    println!("model_path = {}", model_path.display());
    println!("skill_path = {}", skill_path.display());
    println!("bootstrap_path = {}", bootstrap_path.display());
    println!("config_created = {}", created);

    Ok(())
}

fn install_binary() -> Result<std::path::PathBuf> {
    let source = current_executable()?;
    let target = install_binary_path()?;

    if source == target {
        return Ok(target);
    }

    fs::copy(&source, &target).with_context(|| {
        format!(
            "failed to copy lilaccaps binary from {} to {}",
            source.display(),
            target.display()
        )
    })?;

    let metadata = fs::metadata(&target)
        .with_context(|| format!("failed to inspect installed binary at {}", target.display()))?;
    let mut permissions = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
        fs::set_permissions(&target, permissions).with_context(|| {
            format!(
                "failed to set executable permissions on installed binary {}",
                target.display()
            )
        })?;
    }

    let path = env::var_os("PATH").unwrap_or_default();
    let cargo_bin = cargo_bin_dir()?;
    let on_path = env::split_paths(&path).any(|entry| entry == cargo_bin);
    if !on_path {
        eprintln!("warning: {} is not currently on PATH", cargo_bin.display());
    }

    Ok(target)
}
