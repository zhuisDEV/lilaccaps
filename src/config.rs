use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::integration::default_skill_path;
use crate::release::infer_github_repo;
use crate::runtime::ensure_dir;

const DEFAULT_RUNTIME_HOME: &str = "~/.lilac/lilaccaps";
const CONFIG_FILE_NAME: &str = "lilaccaps.toml";

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub config_path: PathBuf,
    pub runtime_home: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub paths: ConfigPaths,
    pub config: Config,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub runtime: RuntimeConfig,
    pub agent: AgentConfig,
    pub release: ReleaseConfig,
    pub transcribe: TranscribeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub home: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub skill_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseConfig {
    pub github_repo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeConfig {
    #[serde(default = "default_transcribe_language")]
    pub language: String,
    pub model: ModelConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub path: Option<PathBuf>,
}

pub fn load_or_init_config(override_path: Option<PathBuf>) -> Result<LoadedConfig> {
    let config_path = override_path.unwrap_or(default_config_path()?);
    let created = !config_path.exists();

    if created {
        let config = default_config()?;
        write_config_file(&config_path, &config)?;
    }

    let raw = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read config file {}", config_path.display()))?;
    let mut config: Config = toml::from_str(&raw)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    if let Ok(home) = env::var("LILACCAPS_HOME") {
        config.runtime.home = expand_home(Path::new(&home))?;
    }

    let paths = ConfigPaths {
        config_path,
        runtime_home: config.runtime.home.clone(),
    };

    Ok(LoadedConfig {
        paths,
        config,
        created,
    })
}

pub fn load_config(override_path: Option<PathBuf>) -> Result<LoadedConfig> {
    let config_path = override_path.unwrap_or(default_config_path()?);
    let created = false;
    let config = if config_path.exists() {
        let raw = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read config file {}", config_path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse {}", config_path.display()))?
    } else {
        default_config()?
    };

    let mut config = config;
    if let Ok(home) = env::var("LILACCAPS_HOME") {
        config.runtime.home = expand_home(Path::new(&home))?;
    }

    let paths = ConfigPaths {
        config_path,
        runtime_home: config.runtime.home.clone(),
    };

    Ok(LoadedConfig {
        paths,
        config,
        created,
    })
}

pub fn default_config() -> Result<Config> {
    let runtime_home = default_runtime_home()?;
    let skill_path = default_skill_path()?;
    let github_repo = infer_github_repo();

    Ok(Config {
        runtime: RuntimeConfig { home: runtime_home },
        agent: AgentConfig { skill_path },
        release: ReleaseConfig { github_repo },
        transcribe: TranscribeConfig {
            language: default_transcribe_language(),
            model: ModelConfig {
                id: "base".to_string(),
                path: None,
            },
        },
    })
}

pub fn default_runtime_home() -> Result<PathBuf> {
    expand_home(Path::new(DEFAULT_RUNTIME_HOME))
}

pub fn default_transcribe_language() -> String {
    "auto".to_string()
}

pub fn default_config_path() -> Result<PathBuf> {
    Ok(default_runtime_home()?.join(CONFIG_FILE_NAME))
}

pub fn write_config_file(config_path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = config_path.parent() {
        ensure_dir(parent)?;
    }

    let rendered = toml::to_string_pretty(config).context("failed to render lilaccaps.toml")?;
    fs::write(config_path, rendered)
        .with_context(|| format!("failed to write config file {}", config_path.display()))
}

pub fn expand_home(path: &Path) -> Result<PathBuf> {
    let path_str = path.to_string_lossy();
    if let Some(stripped) = path_str.strip_prefix("~/") {
        let home = dirs::home_dir().context("failed to detect home directory")?;
        return Ok(home.join(stripped));
    }

    if path_str == "~" {
        return dirs::home_dir().context("failed to detect home directory");
    }

    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Config, default_config, default_config_path, default_runtime_home, expand_home};

    #[test]
    fn expands_tilde_paths() {
        let expanded = expand_home(Path::new("~/demo")).expect("tilde path should expand");
        assert!(expanded.ends_with("demo"));
        assert!(expanded.is_absolute());
    }

    #[test]
    fn default_config_lives_under_runtime_home() {
        let runtime_home = default_runtime_home().expect("runtime home should resolve");
        let config_path = default_config_path().expect("config path should resolve");
        assert_eq!(config_path, runtime_home.join("lilaccaps.toml"));
    }

    #[test]
    fn default_config_uses_auto_language_detection() {
        let config = default_config().expect("default config should build");
        assert_eq!(config.transcribe.language, "auto");
    }

    #[test]
    fn older_config_without_language_still_parses() {
        let raw = r#"
[runtime]
home = "/tmp/lilaccaps"

[agent]
skill_path = "/tmp/SKILL.md"

[release]
github_repo = "zhuisDEV/lilaccaps"

[transcribe.model]
id = "base"
"#;

        let config: Config = toml::from_str(raw).expect("older config should parse");
        assert_eq!(config.transcribe.language, "auto");
    }
}
