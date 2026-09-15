use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{AgentConfig, Config, ConfigPaths, expand_home};
use crate::runtime::atomic_write;

const GENERATED_SKILL_MARKER: &str = "<!-- generated-by-lilaccaps -->";

pub fn default_skill_path() -> Result<PathBuf> {
    if let Ok(openclaw_home) = env::var("OPENCLAW_HOME") {
        return expand_home(
            &PathBuf::from(openclaw_home)
                .join("skills")
                .join("lilaccaps")
                .join("SKILL.md"),
        );
    }

    if let Ok(codex_home) = env::var("CODEX_HOME") {
        return expand_home(&PathBuf::from(codex_home).join("skills/lilaccaps/SKILL.md"));
    }
    expand_home(std::path::Path::new("~/.codex/skills/lilaccaps/SKILL.md"))
}

pub fn detect_skill_path(agent: &AgentConfig) -> Option<PathBuf> {
    let configured = &agent.skill_path;
    if configured.exists() {
        return Some(configured.clone());
    }

    let default = default_skill_path().ok()?;
    if default.exists() {
        return Some(default);
    }

    None
}

pub fn write_bootstrap_markdown(paths: &ConfigPaths, config: &Config) -> Result<PathBuf> {
    let bootstrap_path = paths.runtime_home.join("bootstrap.md");
    let content = format!(
        "# LilacCaps bootstrap\n\nConfig: `{}`\n\nRuntime: `{}`\n\nSkill: `{}`\n\n\
Use `lilaccaps status` and `lilaccaps doctor` to inspect readiness. On macOS, \
`doctor --fix` repairs mapped Homebrew dependencies. On externally managed systems, \
use `update --skip-dependencies` when updating.\n\n\
Caption projects use `workflow start`, `resume`, `status`, `accept` and `render`. \
Generation and verification send text to authenticated Codex through the existing ChatGPT login. \
Check authentication with `codex login status`. Review the generated SRT and reports before \
acceptance, then inspect the rendered video. Text-only review is not an audio audit.\n\n\
Use `watermark-preset save/list/show/remove` for reusable text/image marks. Saved images and explicit \
font files are copied into the runtime's `watermarks/` folder; project snapshots survive removal \
of the library preset.\n\n\
See the configured skill for the complete workflow and the repository README for settings.\n",
        paths.config_path.display(),
        paths.runtime_home.display(),
        config.agent.skill_path.display()
    );
    atomic_write(&bootstrap_path, content)?;
    Ok(bootstrap_path)
}

pub fn ensure_skill_file(config: &Config) -> Result<PathBuf> {
    let skill_path = &config.agent.skill_path;
    if skill_path.exists() {
        let raw = fs::read_to_string(skill_path)
            .with_context(|| format!("failed to inspect skill file {}", skill_path.display()))?;
        if !raw.contains(GENERATED_SKILL_MARKER) {
            return Ok(skill_path.clone());
        }
    }
    // Bundle the same reviewed instructions used in the repo; lifecycle updates
    // refresh generated copies while preserving customised skills and manuals.
    let content = format!(
        "{}\n{GENERATED_SKILL_MARKER}\n",
        include_str!("../SKILL.md")
    );
    atomic_write(skill_path, content)?;
    let manual = skill_path.with_file_name("README.md");
    let refresh_manual =
        !manual.exists() || fs::read_to_string(&manual)?.contains(GENERATED_SKILL_MARKER);
    if refresh_manual {
        atomic_write(
            &manual,
            format!("{GENERATED_SKILL_MARKER}\n{}", include_str!("../README.md")),
        )?;
    }
    Ok(skill_path.clone())
}

pub fn remove_generated_skill_file(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to inspect skill file {}", path.display()))?;
    if !raw.contains(GENERATED_SKILL_MARKER) {
        return Ok(false);
    }

    fs::remove_file(path)
        .with_context(|| format!("failed to remove generated skill file {}", path.display()))?;
    let manual = path.with_file_name("README.md");
    if manual.exists() && fs::read_to_string(&manual)?.contains(GENERATED_SKILL_MARKER) {
        fs::remove_file(&manual)?;
    }
    Ok(true)
}
