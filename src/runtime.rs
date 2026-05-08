use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::config::{Config, ConfigPaths};

#[derive(Debug, Clone, Copy)]
pub struct CommandDependency {
    pub name: &'static str,
    pub purpose: &'static str,
    pub install_hint: &'static str,
    pub brew_package: Option<&'static str>,
}

pub const CARGO_DEPENDENCY: CommandDependency = CommandDependency {
    name: "cargo",
    purpose: "build and update the lilaccaps binary from source",
    install_hint: "Install the Rust toolchain from https://rustup.rs",
    brew_package: None,
};

pub const FFMPEG_DEPENDENCY: CommandDependency = CommandDependency {
    name: "ffmpeg",
    purpose: "extract audio and render video output",
    install_hint: "On macOS with Homebrew: brew install ffmpeg",
    brew_package: Some("ffmpeg"),
};

pub const FFPROBE_DEPENDENCY: CommandDependency = CommandDependency {
    name: "ffprobe",
    purpose: "inspect media streams and dimensions",
    install_hint: "On macOS with Homebrew: brew install ffmpeg",
    brew_package: Some("ffmpeg"),
};

pub const CMAKE_DEPENDENCY: CommandDependency = CommandDependency {
    name: "cmake",
    purpose: "build whisper-rs and whisper.cpp during cargo install/update",
    install_hint: "On macOS with Homebrew: brew install cmake",
    brew_package: Some("cmake"),
};

pub const MAGICK_DEPENDENCY: CommandDependency = CommandDependency {
    name: "magick",
    purpose: "render fallback subtitle overlays when ffmpeg lacks the subtitles filter",
    install_hint: "On macOS with Homebrew: brew install imagemagick",
    brew_package: Some("imagemagick"),
};

const BREW_DEPENDENCY: CommandDependency = CommandDependency {
    name: "brew",
    purpose: "install missing lilaccaps prerequisites automatically",
    install_hint: "Install Homebrew from https://brew.sh and rerun lilaccaps doctor --fix",
    brew_package: None,
};

const ALL_DEPENDENCIES: [CommandDependency; 5] = [
    CARGO_DEPENDENCY,
    FFMPEG_DEPENDENCY,
    FFPROBE_DEPENDENCY,
    CMAKE_DEPENDENCY,
    MAGICK_DEPENDENCY,
];

#[derive(Debug, Clone)]
pub struct RuntimeHealth {
    pub installed: bool,
    pub config_valid: bool,
    pub healthy: bool,
    pub cargo_available: bool,
    pub ffmpeg_available: bool,
    pub ffprobe_available: bool,
    pub cmake_available: bool,
    pub magick_available: bool,
    pub build_ready: bool,
    pub fallback_renderer_ready: bool,
    pub model_ready: bool,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DependencyStatus {
    pub dependency: CommandDependency,
    pub available: bool,
}

#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub statuses: Vec<DependencyStatus>,
    pub missing_commands: Vec<String>,
    pub advisories: Vec<String>,
    pub brew_packages: Vec<String>,
    pub can_fix_with_brew: bool,
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory {}", path.display()))
}

pub fn cargo_bin_dir() -> Result<PathBuf> {
    if let Ok(cargo_home) = env::var("CARGO_HOME") {
        return Ok(PathBuf::from(cargo_home).join("bin"));
    }

    let home = dirs::home_dir().context("failed to detect home directory")?;
    Ok(home.join(".cargo").join("bin"))
}

pub fn install_binary_path() -> Result<PathBuf> {
    Ok(cargo_bin_dir()?.join("lilaccaps"))
}

pub fn current_executable() -> Result<PathBuf> {
    env::current_exe().context("failed to locate current lilaccaps executable")
}

pub fn models_dir(runtime_home: &Path) -> PathBuf {
    runtime_home.join("models")
}

pub fn tmp_dir(runtime_home: &Path) -> PathBuf {
    runtime_home.join("tmp")
}

pub fn command_exists(name: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|dir| {
            let candidate = dir.join(name);
            candidate.is_file()
        })
    })
}

pub fn ensure_dependency(dep: CommandDependency) -> Result<()> {
    if command_exists(dep.name) {
        return Ok(());
    }

    bail!(
        "{} is required to {} but was not found on PATH. {}",
        dep.name,
        dep.purpose,
        dep.install_hint
    );
}

pub fn collect_dependency_statuses() -> Vec<DependencyStatus> {
    ALL_DEPENDENCIES
        .iter()
        .copied()
        .map(|dependency| DependencyStatus {
            dependency,
            available: command_exists(dependency.name),
        })
        .collect()
}

pub fn collect_doctor_report() -> DoctorReport {
    let statuses = collect_dependency_statuses();
    let mut missing_commands = Vec::new();
    let mut advisories = Vec::new();
    let mut brew_packages = Vec::new();

    for status in &statuses {
        if status.available {
            continue;
        }

        missing_commands.push(status.dependency.name.to_string());
        advisories.push(format!(
            "{} is required to {} but was not found on PATH. {}",
            status.dependency.name, status.dependency.purpose, status.dependency.install_hint
        ));

        if let Some(package) = status.dependency.brew_package
            && !brew_packages.iter().any(|item| item == package)
        {
            brew_packages.push(package.to_string());
        }
    }

    let can_fix_with_brew = cfg!(target_os = "macos")
        && command_exists(BREW_DEPENDENCY.name)
        && !brew_packages.is_empty();

    DoctorReport {
        statuses,
        missing_commands,
        advisories,
        brew_packages,
        can_fix_with_brew,
    }
}

pub fn fix_missing_dependencies_with_brew(report: &DoctorReport) -> Result<Vec<String>> {
    if report.brew_packages.is_empty() {
        return Ok(Vec::new());
    }

    if !cfg!(target_os = "macos") {
        bail!("automatic prerequisite installation is only supported on macOS with Homebrew");
    }

    ensure_dependency(BREW_DEPENDENCY)?;

    let status = Command::new("brew")
        .arg("install")
        .args(&report.brew_packages)
        .status()
        .context("failed to start Homebrew while installing prerequisites")?;

    if !status.success() {
        bail!(
            "brew install failed while installing prerequisites: {}",
            report.brew_packages.join(" ")
        );
    }

    Ok(report.brew_packages.clone())
}

pub fn detect_runtime_health(paths: &ConfigPaths, config: &Config) -> RuntimeHealth {
    let install_path = install_binary_path().ok();
    let installed = install_path.as_ref().is_some_and(|path| path.exists());
    let cargo_available = command_exists(CARGO_DEPENDENCY.name);
    let ffmpeg_available = command_exists(FFMPEG_DEPENDENCY.name);
    let ffprobe_available = command_exists(FFPROBE_DEPENDENCY.name);
    let cmake_available = command_exists(CMAKE_DEPENDENCY.name);
    let magick_available = command_exists(MAGICK_DEPENDENCY.name);
    let model_path = config
        .transcribe
        .model
        .path
        .clone()
        .unwrap_or_else(|| models_dir(&paths.runtime_home).join("ggml-base.bin"));
    let model_ready = model_path.exists();

    let mut missing = Vec::new();
    if !paths.config_path.exists() {
        missing.push("config".to_string());
    }
    if !paths.runtime_home.exists() {
        missing.push("runtime_home".to_string());
    }
    if !config.agent.skill_path.exists() {
        missing.push("skill_path".to_string());
    }
    if !ffmpeg_available {
        missing.push("ffmpeg".to_string());
    }
    if !ffprobe_available {
        missing.push("ffprobe".to_string());
    }
    if !model_ready {
        missing.push("model".to_string());
    }

    RuntimeHealth {
        installed,
        config_valid: missing.iter().all(|item| item != "config"),
        healthy: installed && missing.is_empty(),
        cargo_available,
        ffmpeg_available,
        ffprobe_available,
        cmake_available,
        magick_available,
        build_ready: cargo_available && cmake_available,
        fallback_renderer_ready: magick_available,
        model_ready,
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandDependency, DoctorReport, ensure_dependency, fix_missing_dependencies_with_brew,
    };

    #[test]
    fn missing_dependency_error_includes_install_hint() {
        let missing = CommandDependency {
            name: "definitely-not-a-real-command-for-lilaccaps-tests",
            purpose: "exercise dependency error messaging",
            install_hint: "Install it with the package manager used for this environment",
            brew_package: None,
        };
        let err = ensure_dependency(missing).unwrap_err();
        let message = err.to_string();
        assert!(message.contains(missing.name));
        assert!(message.contains(missing.install_hint));
    }

    #[test]
    fn brew_fix_rejects_non_macos_without_running_brew() {
        if cfg!(target_os = "macos") {
            return;
        }

        let report = DoctorReport {
            statuses: Vec::new(),
            missing_commands: vec!["ffmpeg".to_string()],
            advisories: vec!["ffmpeg missing".to_string()],
            brew_packages: vec!["ffmpeg".to_string()],
            can_fix_with_brew: false,
        };

        let err = fix_missing_dependencies_with_brew(&report).unwrap_err();
        assert!(
            err.to_string()
                .contains("only supported on macOS with Homebrew")
        );
    }
}
