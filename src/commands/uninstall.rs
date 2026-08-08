use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::cli::UninstallArgs;
use crate::config::load_config;
use crate::integration::remove_generated_skill_file;
use crate::runtime::{install_binary_path, validate_runtime_home_for_removal};

pub fn run(args: UninstallArgs) -> Result<()> {
    let loaded = load_config(args.config_path)?;
    let binary_path = install_binary_path()?;

    if !args.yes {
        bail!(
            "refusing to remove lilaccaps without --yes\nbinary_path = {}\nconfig_path = {}\nruntime_home = {}",
            binary_path.display(),
            loaded.paths.config_path.display(),
            loaded.paths.runtime_home.display()
        );
    }

    let validated_runtime_home = if path_entry_exists(&loaded.paths.runtime_home)? {
        Some(validate_runtime_home_for_removal(
            &loaded.paths.runtime_home,
        )?)
    } else {
        None
    };

    let removed_skill = remove_generated_skill_file(&loaded.config.agent.skill_path)?;

    if let Some(runtime_home) = validated_runtime_home {
        fs::remove_dir_all(&runtime_home)
            .with_context(|| format!("failed to remove runtime home {}", runtime_home.display()))?;
    }

    if path_entry_exists(&loaded.paths.config_path)? {
        fs::remove_file(&loaded.paths.config_path).with_context(|| {
            format!(
                "failed to remove config file {}",
                loaded.paths.config_path.display()
            )
        })?;
    }

    if path_entry_exists(&binary_path)? {
        fs::remove_file(&binary_path).with_context(|| {
            format!(
                "failed to remove installed binary {}",
                binary_path.display()
            )
        })?;
    }

    println!("uninstalled = true");
    println!("binary_path = {}", binary_path.display());
    println!("config_path = {}", loaded.paths.config_path.display());
    println!("runtime_home = {}", loaded.paths.runtime_home.display());
    println!("skill_removed = {}", removed_skill);

    Ok(())
}

fn path_entry_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to inspect {}", path.display())),
    }
}
