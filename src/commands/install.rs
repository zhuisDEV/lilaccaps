use std::env;
use std::fs;

use anyhow::{Context, Result, bail};

use crate::cli::InstallArgs;
use crate::config::{LoadedConfig, TranscribeEngine, load_or_init_config};
use crate::faster_whisper;
use crate::integration::{ensure_skill_file, write_bootstrap_markdown};
use crate::model::ensure_model_downloaded;
use crate::runtime::{
    cargo_bin_dir, collect_doctor_report_for_config, current_executable, ensure_cleanup_command,
    ensure_dir, ensure_runtime_marker, fix_dependencies_with_brew, install_binary_path, models_dir,
    tmp_dir,
};

pub fn run(args: InstallArgs) -> Result<()> {
    let LoadedConfig {
        paths,
        config,
        created,
    } = load_or_init_config(args.config_path)?;
    let mut report = collect_doctor_report_for_config(&config);
    let mut fixed_packages = Vec::new();
    if args.fix && !report.brew_packages.is_empty() {
        fixed_packages = fix_dependencies_with_brew(&report)?;
        report = collect_doctor_report_for_config(&config);
    }

    if !report.missing_commands.is_empty() {
        let hint = if report.can_fix_with_brew {
            format!(
                "run `lilaccaps install --fix` or `brew install {}`",
                report.brew_packages.join(" ")
            )
        } else if cfg!(target_os = "macos") {
            "install Homebrew from https://brew.sh or install the missing prerequisites manually"
                .to_string()
        } else {
            "install the missing prerequisites with your platform package manager".to_string()
        };
        bail!(
            "prerequisite check failed for: {}. To continue, {}.",
            report.missing_commands.join(", "),
            hint
        );
    }

    ensure_dir(&paths.runtime_home)?;
    ensure_runtime_marker(&paths.runtime_home)?;
    ensure_dir(&models_dir(&paths.runtime_home))?;
    ensure_dir(&tmp_dir(&paths.runtime_home))?;
    ensure_dir(&cargo_bin_dir()?)?;

    let bootstrap_path = write_bootstrap_markdown(&paths, &config)?;
    let binary_path = install_binary()?;
    let model_path = ensure_model_downloaded(&paths, &config)?;
    if config.transcribe.engine == TranscribeEngine::FasterWhisper {
        faster_whisper::check(&paths.runtime_home)?;
    }
    if config.transcribe.cleanup.enabled {
        ensure_cleanup_command(&config.transcribe.cleanup.command)?;
    }
    let skill_path = ensure_skill_file(&config)?;

    println!("installed = true");
    println!("binary_path = {}", binary_path.display());
    println!("config_path = {}", paths.config_path.display());
    println!("runtime_home = {}", paths.runtime_home.display());
    println!("model_path = {}", model_path.display());
    println!("skill_path = {}", skill_path.display());
    println!("bootstrap_path = {}", bootstrap_path.display());
    println!("config_created = {}", created);
    println!(
        "fixed_packages = {}",
        if fixed_packages.is_empty() {
            "none".to_string()
        } else {
            fixed_packages.join(", ")
        }
    );

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
