use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::integration::default_skill_path;
use crate::release::{default_github_repo, infer_github_repo};
use crate::runtime::atomic_write;

const DEFAULT_RUNTIME_HOME: &str = "~/.lilac/lilaccaps";
const CONFIG_FILE_NAME: &str = "lilaccaps.toml";
const LEGACY_TRANSLATE_MODEL: &str = "gemini-3.1-flash-lite-preview";
const DEFAULT_TRANSLATE_MODEL: &str = "gemini-3.1-flash-lite";

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
    #[serde(default)]
    pub burnin: BurninConfig,
    #[serde(default)]
    pub translate: TranslateConfig,
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
    #[serde(default)]
    pub engine: TranscribeEngine,
    #[serde(default = "default_transcribe_language")]
    pub language: String,
    #[serde(default)]
    pub segmentation: TranscribeSegmentationConfig,
    #[serde(default)]
    pub cues: TranscribeCueConfig,
    #[serde(default)]
    pub cleanup: TranscribeCleanupConfig,
    pub model: ModelConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TranscribeEngine {
    #[default]
    WhisperRs,
    FasterWhisper,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeSegmentationConfig {
    #[serde(default)]
    pub mode: TranscribeSegmentationMode,
    #[serde(default = "default_transcribe_chunk_seconds")]
    pub chunk_seconds: u64,
    #[serde(default = "default_transcribe_overlap_seconds")]
    pub overlap_seconds: u64,
    #[serde(default = "default_transcribe_min_speech_ms")]
    pub min_speech_ms: u64,
    #[serde(default = "default_transcribe_min_silence_ms")]
    pub min_silence_ms: u64,
    #[serde(default = "default_transcribe_padding_ms")]
    pub padding_ms: u64,
    #[serde(default = "default_transcribe_max_window_seconds")]
    pub max_window_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TranscribeSegmentationMode {
    #[default]
    Speech,
    Fixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeCueConfig {
    #[serde(default = "default_transcribe_min_duration_ms")]
    pub min_duration_ms: u64,
    #[serde(default = "default_transcribe_max_duration_ms")]
    pub max_duration_ms: u64,
    #[serde(default = "default_transcribe_end_padding_ms")]
    pub end_padding_ms: u64,
    #[serde(default = "default_transcribe_pause_split_ms")]
    pub pause_split_ms: u64,
    #[serde(default = "default_transcribe_max_chars_per_line")]
    pub max_chars_per_line: usize,
    #[serde(default = "default_transcribe_max_cjk_chars_per_line")]
    pub max_cjk_chars_per_line: usize,
    #[serde(default = "default_transcribe_max_lines")]
    pub max_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeCleanupConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_transcribe_cleanup_command")]
    pub command: String,
    #[serde(default = "default_transcribe_cleanup_model")]
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurninConfig {
    #[serde(default = "default_burnin_font")]
    pub font: String,
    #[serde(default = "default_burnin_colour", alias = "color")]
    pub colour: String,
    #[serde(default = "default_burnin_size")]
    pub size: u32,
    #[serde(default = "default_burnin_line_spacing")]
    pub line_spacing: u32,
    #[serde(default = "default_burnin_advanced_styling")]
    pub advanced_styling: bool,
    #[serde(default)]
    pub outline: BurninOutlineConfig,
    #[serde(default)]
    pub styles: HashMap<String, BurninLineStyleConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateConfig {
    #[serde(default = "default_translate_model")]
    pub model: String,
    #[serde(default = "default_translate_append")]
    pub append: bool,
    #[serde(default)]
    pub default_targets: Vec<String>,
    #[serde(default)]
    pub line_order: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BurninLineStyleConfig {
    pub font: Option<String>,
    #[serde(alias = "color")]
    pub colour: Option<String>,
    pub size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurninOutlineConfig {
    #[serde(default = "default_burnin_outline_enabled")]
    pub enabled: bool,
    #[serde(default = "default_burnin_outline_colour", alias = "color")]
    pub colour: String,
    #[serde(default = "default_burnin_outline_width")]
    pub width: u32,
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
    let migrated = migrate_managed_defaults(&mut config);
    if migrated {
        write_config_file(&config_path, &config)?;
    }
    resolve_config_paths(&mut config)?;

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
    migrate_managed_defaults(&mut config);
    resolve_config_paths(&mut config)?;

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
    let github_repo = Some(infer_github_repo().unwrap_or_else(default_github_repo));

    Ok(Config {
        runtime: RuntimeConfig { home: runtime_home },
        agent: AgentConfig { skill_path },
        release: ReleaseConfig { github_repo },
        transcribe: TranscribeConfig {
            engine: TranscribeEngine::default(),
            language: default_transcribe_language(),
            segmentation: TranscribeSegmentationConfig::default(),
            cues: TranscribeCueConfig::default(),
            cleanup: TranscribeCleanupConfig::default(),
            model: ModelConfig {
                id: "base".to_string(),
                path: None,
            },
        },
        burnin: BurninConfig {
            font: default_burnin_font(),
            colour: default_burnin_colour(),
            size: default_burnin_size(),
            line_spacing: default_burnin_line_spacing(),
            advanced_styling: default_burnin_advanced_styling(),
            outline: BurninOutlineConfig::default(),
            styles: HashMap::new(),
        },
        translate: TranslateConfig::default(),
    })
}

pub fn default_runtime_home() -> Result<PathBuf> {
    expand_home(Path::new(DEFAULT_RUNTIME_HOME))
}

pub fn default_transcribe_language() -> String {
    "auto".to_string()
}

pub fn default_transcribe_chunk_seconds() -> u64 {
    30
}

pub fn default_transcribe_overlap_seconds() -> u64 {
    2
}

pub fn default_transcribe_min_speech_ms() -> u64 {
    400
}

pub fn default_transcribe_min_silence_ms() -> u64 {
    350
}

pub fn default_transcribe_padding_ms() -> u64 {
    300
}

pub fn default_transcribe_max_window_seconds() -> u64 {
    30
}

pub fn default_transcribe_min_duration_ms() -> u64 {
    800
}

pub fn default_transcribe_max_duration_ms() -> u64 {
    6_000
}

pub fn default_transcribe_end_padding_ms() -> u64 {
    150
}

pub fn default_transcribe_pause_split_ms() -> u64 {
    500
}

pub fn default_transcribe_max_chars_per_line() -> usize {
    42
}

pub fn default_transcribe_max_cjk_chars_per_line() -> usize {
    18
}

pub fn default_transcribe_max_lines() -> usize {
    2
}

pub fn default_transcribe_cleanup_command() -> String {
    "codex".to_string()
}

pub fn default_transcribe_cleanup_model() -> String {
    "gpt-5.6-terra".to_string()
}

pub fn default_burnin_font() -> String {
    "auto".to_string()
}

pub fn default_burnin_colour() -> String {
    "auto".to_string()
}

pub fn default_burnin_size() -> u32 {
    0
}

pub fn default_burnin_line_spacing() -> u32 {
    0
}

pub fn default_burnin_advanced_styling() -> bool {
    true
}

pub fn default_burnin_outline_enabled() -> bool {
    true
}

pub fn default_burnin_outline_colour() -> String {
    "black".to_string()
}

pub fn default_burnin_outline_width() -> u32 {
    2
}

pub fn default_translate_model() -> String {
    DEFAULT_TRANSLATE_MODEL.to_string()
}

pub fn default_translate_append() -> bool {
    true
}

impl Default for BurninConfig {
    fn default() -> Self {
        Self {
            font: default_burnin_font(),
            colour: default_burnin_colour(),
            size: default_burnin_size(),
            line_spacing: default_burnin_line_spacing(),
            advanced_styling: default_burnin_advanced_styling(),
            outline: BurninOutlineConfig::default(),
            styles: HashMap::new(),
        }
    }
}

impl Default for TranscribeSegmentationConfig {
    fn default() -> Self {
        Self {
            mode: TranscribeSegmentationMode::default(),
            chunk_seconds: default_transcribe_chunk_seconds(),
            overlap_seconds: default_transcribe_overlap_seconds(),
            min_speech_ms: default_transcribe_min_speech_ms(),
            min_silence_ms: default_transcribe_min_silence_ms(),
            padding_ms: default_transcribe_padding_ms(),
            max_window_seconds: default_transcribe_max_window_seconds(),
        }
    }
}

impl Default for TranscribeCueConfig {
    fn default() -> Self {
        Self {
            min_duration_ms: default_transcribe_min_duration_ms(),
            max_duration_ms: default_transcribe_max_duration_ms(),
            end_padding_ms: default_transcribe_end_padding_ms(),
            pause_split_ms: default_transcribe_pause_split_ms(),
            max_chars_per_line: default_transcribe_max_chars_per_line(),
            max_cjk_chars_per_line: default_transcribe_max_cjk_chars_per_line(),
            max_lines: default_transcribe_max_lines(),
        }
    }
}

impl Default for TranscribeCleanupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command: default_transcribe_cleanup_command(),
            model: default_transcribe_cleanup_model(),
        }
    }
}

impl Default for BurninOutlineConfig {
    fn default() -> Self {
        Self {
            enabled: default_burnin_outline_enabled(),
            colour: default_burnin_outline_colour(),
            width: default_burnin_outline_width(),
        }
    }
}

impl Default for TranslateConfig {
    fn default() -> Self {
        Self {
            model: default_translate_model(),
            append: default_translate_append(),
            default_targets: Vec::new(),
            line_order: Vec::new(),
        }
    }
}

pub fn default_config_path() -> Result<PathBuf> {
    Ok(default_runtime_home()?.join(CONFIG_FILE_NAME))
}

pub fn write_config_file(config_path: &Path, config: &Config) -> Result<()> {
    let rendered = toml::to_string_pretty(config).context("failed to render lilaccaps.toml")?;
    atomic_write(config_path, rendered)
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

fn migrate_managed_defaults(config: &mut Config) -> bool {
    let mut changed = false;
    if config.translate.model == LEGACY_TRANSLATE_MODEL {
        config.translate.model = default_translate_model();
        changed = true;
    }
    if config
        .release
        .github_repo
        .as_deref()
        .is_none_or(|repo| repo.trim().is_empty())
    {
        config.release.github_repo = Some(default_github_repo());
        changed = true;
    }
    changed
}

fn resolve_config_paths(config: &mut Config) -> Result<()> {
    config.runtime.home = expand_home(&config.runtime.home)?;
    config.agent.skill_path = expand_home(&config.agent.skill_path)?;
    if let Some(model_path) = config.transcribe.model.path.as_mut() {
        *model_path = expand_home(model_path)?;
    }
    if let Ok(home) = env::var("LILACCAPS_HOME") {
        config.runtime.home = expand_home(Path::new(&home))?;
    }
    if !config.runtime.home.is_absolute() {
        anyhow::bail!(
            "runtime.home must be an absolute path or start with `~`: {}",
            config.runtime.home.display()
        );
    }
    if !config.agent.skill_path.is_absolute() {
        anyhow::bail!(
            "agent.skill_path must be an absolute path or start with `~`: {}",
            config.agent.skill_path.display()
        );
    }
    if let Some(model_path) = &config.transcribe.model.path
        && !model_path.is_absolute()
    {
        anyhow::bail!(
            "transcribe.model.path must be an absolute path or start with `~`: {}",
            model_path.display()
        );
    }
    validate_transcribe_config(&config.transcribe)?;
    Ok(())
}

pub fn validate_transcribe_config(config: &TranscribeConfig) -> Result<()> {
    match config.engine {
        TranscribeEngine::WhisperRs => {
            if !matches!(
                config.model.id.as_str(),
                "tiny"
                    | "base"
                    | "small"
                    | "medium"
                    | "tiny.en"
                    | "base.en"
                    | "small.en"
                    | "medium.en"
            ) {
                anyhow::bail!(
                    "unsupported whisper-rs model id: {}; use tiny, base, small, medium, or an .en variant",
                    config.model.id
                );
            }
        }
        TranscribeEngine::FasterWhisper => {
            if config.model.path.is_none()
                && !matches!(config.model.id.as_str(), "large-v3" | "large-v3-turbo")
            {
                anyhow::bail!(
                    "unsupported faster-whisper model id: {}; use large-v3 or large-v3-turbo",
                    config.model.id
                );
            }
        }
    }
    if config.cleanup.enabled {
        if config.cleanup.command.trim().is_empty() {
            anyhow::bail!("transcribe.cleanup.command must not be empty when cleanup is enabled");
        }
        let cleanup_command = Path::new(&config.cleanup.command);
        if cleanup_command.components().count() > 1 && !cleanup_command.is_absolute() {
            anyhow::bail!(
                "transcribe.cleanup.command must be an executable name or absolute path: {}",
                config.cleanup.command
            );
        }
        if config.cleanup.model.trim().is_empty() {
            anyhow::bail!("transcribe.cleanup.model must not be empty when cleanup is enabled");
        }
    }
    let segmentation = &config.segmentation;
    if segmentation.chunk_seconds == 0 {
        anyhow::bail!("transcribe.segmentation.chunk_seconds must be greater than zero");
    }
    if segmentation.overlap_seconds >= segmentation.chunk_seconds {
        anyhow::bail!("transcribe.segmentation.overlap_seconds must be smaller than chunk_seconds");
    }
    if segmentation.max_window_seconds == 0 {
        anyhow::bail!("transcribe.segmentation.max_window_seconds must be greater than zero");
    }
    if segmentation.overlap_seconds >= segmentation.max_window_seconds {
        anyhow::bail!(
            "transcribe.segmentation.overlap_seconds must be smaller than max_window_seconds"
        );
    }
    if segmentation.min_speech_ms == 0 || segmentation.min_silence_ms == 0 {
        anyhow::bail!("transcribe speech and silence durations must be greater than zero");
    }

    let cues = &config.cues;
    if cues.min_duration_ms == 0 {
        anyhow::bail!("transcribe.cues.min_duration_ms must be greater than zero");
    }
    if cues.max_duration_ms < cues.min_duration_ms {
        anyhow::bail!(
            "transcribe.cues.max_duration_ms must be greater than or equal to min_duration_ms"
        );
    }
    if cues.pause_split_ms == 0 {
        anyhow::bail!("transcribe.cues.pause_split_ms must be greater than zero");
    }
    if cues.max_chars_per_line == 0 || cues.max_cjk_chars_per_line == 0 || cues.max_lines == 0 {
        anyhow::bail!("transcribe cue line limits must be greater than zero");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        Config, default_config, default_config_path, default_runtime_home, expand_home,
        migrate_managed_defaults, resolve_config_paths, validate_transcribe_config,
    };

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
        assert_eq!(config.transcribe.engine, super::TranscribeEngine::WhisperRs);
        assert_eq!(config.transcribe.segmentation.chunk_seconds, 30);
        assert_eq!(config.transcribe.segmentation.overlap_seconds, 2);
        assert_eq!(
            config.transcribe.segmentation.mode,
            super::TranscribeSegmentationMode::Speech
        );
        assert_eq!(config.transcribe.segmentation.min_speech_ms, 400);
        assert_eq!(config.transcribe.segmentation.min_silence_ms, 350);
        assert_eq!(config.transcribe.segmentation.padding_ms, 300);
        assert_eq!(config.transcribe.segmentation.max_window_seconds, 30);
        assert_eq!(config.transcribe.cues.min_duration_ms, 800);
        assert_eq!(config.transcribe.cues.max_duration_ms, 6_000);
        assert_eq!(config.transcribe.cues.end_padding_ms, 150);
        assert_eq!(config.transcribe.cues.pause_split_ms, 500);
        assert!(!config.transcribe.cleanup.enabled);
        assert_eq!(config.transcribe.cleanup.command, "codex");
        assert_eq!(config.transcribe.cleanup.model, "gpt-5.6-terra");
        assert_eq!(config.burnin.font, "auto");
        assert_eq!(config.burnin.colour, "auto");
        assert_eq!(config.burnin.size, 0);
        assert_eq!(config.burnin.line_spacing, 0);
        assert!(config.burnin.advanced_styling);
        assert!(config.burnin.outline.enabled);
        assert_eq!(config.burnin.outline.colour, "black");
        assert_eq!(config.burnin.outline.width, 2);
        assert!(config.burnin.styles.is_empty());
        assert_eq!(config.translate.model, "gemini-3.1-flash-lite");
        assert!(config.translate.append);
        assert!(config.translate.default_targets.is_empty());
        assert!(config.translate.line_order.is_empty());
        assert_eq!(
            config.release.github_repo.as_deref(),
            Some("zhuisDEV/lilaccaps")
        );
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

[burnin]
font = "auto"
colour = "auto"
size = 0
line_spacing = 0
advanced_styling = true
styles = {}

[translate]
model = "gemini-3.1-flash-lite-preview"
append = true
default_targets = []
line_order = []

[transcribe.model]
id = "base"
"#;

        let config: Config = toml::from_str(raw).expect("older config should parse");
        assert_eq!(config.transcribe.language, "auto");
        assert_eq!(config.transcribe.engine, super::TranscribeEngine::WhisperRs);
        assert_eq!(config.transcribe.segmentation.chunk_seconds, 30);
        assert_eq!(config.transcribe.segmentation.overlap_seconds, 2);
        assert_eq!(
            config.transcribe.segmentation.mode,
            super::TranscribeSegmentationMode::Speech
        );
        assert_eq!(config.transcribe.cues.max_chars_per_line, 42);
        assert_eq!(config.transcribe.cues.max_cjk_chars_per_line, 18);
        assert_eq!(config.transcribe.cues.max_lines, 2);
        assert_eq!(config.transcribe.cues.pause_split_ms, 500);
        assert!(!config.transcribe.cleanup.enabled);
        assert_eq!(config.burnin.font, "auto");
        assert_eq!(config.burnin.colour, "auto");
        assert_eq!(config.burnin.size, 0);
        assert_eq!(config.burnin.line_spacing, 0);
        assert!(config.burnin.advanced_styling);
        assert!(config.burnin.outline.enabled);
        assert_eq!(config.burnin.outline.colour, "black");
        assert_eq!(config.burnin.outline.width, 2);
        assert!(config.burnin.styles.is_empty());
        assert_eq!(config.translate.model, "gemini-3.1-flash-lite-preview");
        assert!(config.translate.append);
        assert!(config.translate.default_targets.is_empty());
        assert!(config.translate.line_order.is_empty());
    }

    #[test]
    fn default_config_renders_managed_sections() {
        let config = default_config().expect("default config should build");
        let rendered = toml::to_string_pretty(&config).expect("default config should render");
        assert!(rendered.contains("[burnin.outline]"));
        assert!(rendered.contains("enabled = true"));
        assert!(rendered.contains("colour = \"black\""));
        assert!(rendered.contains("width = 2"));
        assert!(rendered.contains("[transcribe.segmentation]"));
        assert!(rendered.contains("mode = \"speech\""));
        assert!(rendered.contains("min_silence_ms = 350"));
        assert!(rendered.contains("[transcribe.cues]"));
        assert!(rendered.contains("[transcribe.cleanup]"));
    }

    #[test]
    fn migrates_legacy_managed_defaults() {
        let mut config = default_config().expect("default config should build");
        config.translate.model = "gemini-3.1-flash-lite-preview".to_string();
        config.release.github_repo = None;

        assert!(migrate_managed_defaults(&mut config));
        assert_eq!(config.translate.model, "gemini-3.1-flash-lite");
        assert_eq!(
            config.release.github_repo.as_deref(),
            Some("zhuisDEV/lilaccaps")
        );
        assert!(!migrate_managed_defaults(&mut config));
    }

    #[test]
    fn rejects_relative_managed_paths() {
        let mut config = default_config().expect("default config should build");
        config.runtime.home = "relative/runtime".into();
        let error = resolve_config_paths(&mut config)
            .expect_err("relative runtime path should be rejected");
        assert!(
            error
                .to_string()
                .contains("runtime.home must be an absolute path")
        );
    }

    #[test]
    fn rejects_overlap_that_consumes_the_entire_chunk() {
        let mut config = default_config().expect("default config should build");
        config.transcribe.segmentation.overlap_seconds = 30;

        let error = validate_transcribe_config(&config.transcribe)
            .expect_err("invalid overlap should be rejected");
        assert!(error.to_string().contains("overlap_seconds"));
    }

    #[test]
    fn rejects_inverted_cue_duration_limits() {
        let mut config = default_config().expect("default config should build");
        config.transcribe.cues.min_duration_ms = 2_000;
        config.transcribe.cues.max_duration_ms = 1_000;

        let error = validate_transcribe_config(&config.transcribe)
            .expect_err("inverted cue durations should be rejected");
        assert!(error.to_string().contains("max_duration_ms"));
    }

    #[test]
    fn rejects_zero_pause_split_threshold() {
        let mut config = default_config().expect("default config should build");
        config.transcribe.cues.pause_split_ms = 0;

        let error = validate_transcribe_config(&config.transcribe)
            .expect_err("zero pause threshold should be rejected");
        assert!(error.to_string().contains("pause_split_ms"));
    }

    #[test]
    fn rejects_relative_cleanup_command_paths() {
        let mut config = default_config().expect("default config should build");
        config.transcribe.cleanup.enabled = true;
        config.transcribe.cleanup.command = "tools/codex".to_string();

        let error = validate_transcribe_config(&config.transcribe)
            .expect_err("relative cleanup command path should be rejected");
        assert!(
            error
                .to_string()
                .contains("executable name or absolute path")
        );
    }
}
