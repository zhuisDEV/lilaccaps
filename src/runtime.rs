use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{Config, ConfigPaths};

#[derive(Debug, Clone)]
pub struct RuntimeHealth {
    pub installed: bool,
    pub config_valid: bool,
    pub healthy: bool,
    pub ffmpeg_available: bool,
    pub ffprobe_available: bool,
    pub model_ready: bool,
    pub missing: Vec<String>,
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

pub fn detect_runtime_health(paths: &ConfigPaths, config: &Config) -> RuntimeHealth {
    let install_path = install_binary_path().ok();
    let installed = install_path.as_ref().is_some_and(|path| path.exists());
    let ffmpeg_available = command_exists("ffmpeg");
    let ffprobe_available = command_exists("ffprobe");
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
        ffmpeg_available,
        ffprobe_available,
        model_ready,
        missing,
    }
}
