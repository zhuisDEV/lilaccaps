use std::fs;

use anyhow::{Result, bail};

use crate::cli::UninstallArgs;
use crate::config::load_config;
use crate::integration::remove_generated_skill_file;
use crate::runtime::install_binary_path;

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

    if binary_path.exists() {
        fs::remove_file(&binary_path)?;
    }

    if loaded.paths.config_path.exists() {
        fs::remove_file(&loaded.paths.config_path)?;
    }

    if loaded.paths.runtime_home.exists() {
        fs::remove_dir_all(&loaded.paths.runtime_home)?;
    }
    let removed_skill = remove_generated_skill_file(&loaded.config.agent.skill_path)?;

    println!("uninstalled = true");
    println!("binary_path = {}", binary_path.display());
    println!("config_path = {}", loaded.paths.config_path.display());
    println!("runtime_home = {}", loaded.paths.runtime_home.display());
    println!("skill_removed = {}", removed_skill);

    Ok(())
}
