use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{AgentConfig, Config, ConfigPaths, expand_home};
use crate::runtime::ensure_dir;

const GENERATED_SKILL_MARKER: &str = "<!-- generated-by-lilaccaps -->";

pub fn default_skill_path() -> Result<PathBuf> {
    if let Ok(openclaw_home) = env::var("OPENCLAW_HOME") {
        return Ok(PathBuf::from(openclaw_home)
            .join("skills")
            .join("lilaccaps")
            .join("SKILL.md"));
    }

    expand_home(std::path::Path::new(
        "~/.openclaw/skills/lilaccaps/SKILL.md",
    ))
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
        "# lilaccaps bootstrap\n\n\
## Config\n\
- config: `{}`\n\
- runtime home: `{}`\n\
- skill path: `{}`\n\n\
## Required dependencies\n\
- Rust toolchain with `cargo`\n\
- `ffmpeg`\n\
- `ffprobe`\n\
- `cmake`\n\
- ImageMagick `magick`\n\n\
## Why they are needed\n\
- `cargo`: build and install the `lilaccaps` binary\n\
- `ffmpeg`: extract audio and render final video output\n\
- `ffprobe`: inspect media streams and video dimensions\n\
- `cmake`: build `whisper-rs` and its native `whisper.cpp` dependency\n\
- `magick`: fallback burn-in renderer when the local `ffmpeg` build does not include the `subtitles` filter\n\n\
## Install guidance\n\
- macOS with Homebrew: `brew install ffmpeg-full cmake imagemagick`\n\
- if you keep a plain `ffmpeg` build, transcription still works and burn-in falls back to ImageMagick when the `subtitles` or `ass` filters are missing\n\
- after the binary is installed, `lilaccaps doctor --fix` can install the minimum mapped Homebrew packages automatically\n\
- confirm tool availability before use:\n\
  - `cargo --version`\n\
  - `ffmpeg -version`\n\
  - `ffprobe -version`\n\
  - `cmake --version`\n\
  - `magick -version`\n\n\
## Runtime assets\n\
- Whisper model is managed under `{}/models`\n\
- temporary working files are stored under `{}/tmp`\n\
- config is stored in `lilaccaps.toml`\n\n\
## API credentials\n\
- create a local `.env` file under the runtime home or export `GEMINI_API_KEY`\n\
- `lilaccaps translate` loads `GEMINI_API_KEY` from the environment or `$LILACCAPS_HOME/.env`\n\
\n\
## OpenClaw setup\n\
1. Confirm `LILACCAPS_HOME` if you need a non-default runtime directory.\n\
2. Place or link the lilaccaps skill at the configured skill path, or let `lilaccaps install` generate it.\n\
3. Run `lilaccaps install` to initialize config, runtime folders, the model, and the generated skill file.\n\
4. Run `lilaccaps doctor` or `lilaccaps status` to verify config and integration health.\n\n\
## Transcription language behavior\n\
- `transcribe.language = \"auto\"` samples the first 30 seconds for language detection and then transcribes with the detected language explicitly\n\
- `lilaccaps transcribe --lang <code>` forces a language for that run\n\
- if a forced language yields no subtitle text, `lilaccaps` retries greedy decoding and can fall back to the detected language when it differs\n\n\
## Burn-in style behavior\n\
- `burnin.advanced_styling = true` enables per-line styling, custom spacing, and configured custom colours; set it to `false` to ignore those configured advanced settings and prefer the primary ffmpeg subtitle path when available; CLI `--colour` still overrides config for one run\n\
- `burnin.font = \"auto\"` lets the renderer choose a suitable font, or you can force one with `lilaccaps burnin --font <name>`\n\
- `burnin.colour = \"auto\"` keeps the renderer default colour, or you can force one with `lilaccaps burnin --colour <value>`; explicit colour values use the overlay renderer so the chosen fill colour is honored\n\
- `burnin.size = 0` means auto-size from video height, or you can force a point size with `lilaccaps burnin --size <points>`\n\
- `burnin.line_spacing = 0` means auto spacing; set a positive value in `lilaccaps.toml` to control the gap between lines in multiline subtitles\n\
- `lilaccaps burnin` reports `renderer` plus `renderer_reason` so you can see when advanced styling forces the overlay fallback\n\
\n\
## Watermark notes\n\
- `lilaccaps watermark <video> --text <text>` applies a text watermark\n\
- `lilaccaps watermark <video> --image <path>` applies an image watermark\n\
- watermark options include `--position`, `--opacity`, `--size`, and `--margin`; image `--size` is target width, text `--size` is font size\n\
- text watermarks require the `ffmpeg` `drawtext` filter, and image watermarks require the `overlay` filter\n\
\n\
## Translation behavior\n\
- `lilaccaps translate --to en --to ja --append` appends one translated line per target language under the original cue text\n\
- `translate.default_targets` can define default `--to` languages in `lilaccaps.toml`\n\
- `translate.line_order` controls top-to-bottom language order inside each multilingual cue, for example `source`, `ja`, then `en`\n\
- cue timing and indexes stay unchanged during translation\n\
\n\
## Per-language burn-in styling\n\
- `burnin.styles.<role>.font`, `burnin.styles.<role>.colour`, and `burnin.styles.<role>.size` can style individual lines such as `source`, `en`, or `ja`\n\
- burn-in maps cue lines to these roles using `translate.line_order`\n\
\n\
## Expected healthy status\n\
- installed = true\n\
- cargo_available = true\n\
- ffmpeg_available = true\n\
- ffprobe_available = true\n\
- cmake_available = true\n\
- build_ready = true\n\
- can_fix_with_brew = true or false depending on whether Homebrew is available\n\
- model_ready = true\n\
- missing = none\n\
- brew_packages = none\n\
- advisories = none\n",
        paths.config_path.display(),
        paths.runtime_home.display(),
        config.agent.skill_path.display(),
        paths.runtime_home.display(),
        paths.runtime_home.display()
    );

    fs::write(&bootstrap_path, content).with_context(|| {
        format!(
            "failed to write bootstrap instructions to {}",
            bootstrap_path.display()
        )
    })?;

    Ok(bootstrap_path)
}

pub fn ensure_skill_file(config: &Config) -> Result<PathBuf> {
    let skill_path = &config.agent.skill_path;
    if skill_path.exists() {
        return Ok(skill_path.clone());
    }

    if let Some(parent) = skill_path.parent() {
        ensure_dir(parent)?;
    }

    let content = format!(
        "{GENERATED_SKILL_MARKER}\n# lilaccaps\n\nUse the `lilaccaps` CLI for transcription, subtitle rendering, and video watermarks.\n\n## Commands\n- `lilaccaps doctor`\n- `lilaccaps status`\n- `lilaccaps transcribe <input>`\n- `lilaccaps transcribe <input> --lang <code>`\n- `lilaccaps translate <input.srt> --to en --to ja --append`\n- `lilaccaps burnin <video> --subs <subtitle-file>`\n- `lilaccaps burnin <video> --subs <subtitle-file> --colour \"#ffd54f\" --size 42`\n- `lilaccaps watermark <video> --text \"lilac\"`\n- `lilaccaps watermark <video> --image <logo.png> --opacity 0.45 --size 180`\n\n## Notes\n- `burnin` is render-only.\n- `watermark` is render-only and accepts exactly one of `--text` or `--image`.\n- transcription is a separate workflow.\n- `translate` can append one or more target-language lines to each cue for multilingual subtitles.\n- `transcribe.language = \"auto\"` samples the first 30 seconds for language detection and then transcribes with that detected language explicitly.\n- `burnin.font = \"auto\"` lets the renderer choose a suitable font, `burnin.colour = \"auto\"` keeps the renderer default colour, and `burnin.size = 0` auto-scales captions from video height.\n- watermark `--position` supports `top-left`, `top-right`, `bottom-left`, `bottom-right`, and `center`.\n- runtime home is configured via `lilaccaps.toml` and `LILACCAPS_HOME`.\n"
    );

    fs::write(skill_path, content)
        .with_context(|| format!("failed to write skill file {}", skill_path.display()))?;

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
    Ok(true)
}
