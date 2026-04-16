use std::fs;
use std::path::{Path, PathBuf};

use std::collections::HashMap;

use anyhow::{Context, Result, bail};

use crate::config::load_config;
use crate::render::{BurninStyle, LineStyle, burn_in_subtitles};

#[derive(Debug, Clone)]
pub struct BurninOutput {
    pub video: PathBuf,
    pub subs: PathBuf,
    pub output: PathBuf,
    pub font: String,
    pub size: u32,
    pub line_spacing: u32,
    pub status: &'static str,
}

pub fn run(
    video: PathBuf,
    config_path: Option<PathBuf>,
    subs: PathBuf,
    output: Option<PathBuf>,
    font: Option<String>,
    size: Option<u32>,
) -> Result<BurninOutput> {
    if !video.exists() {
        bail!("video input does not exist: {}", video.display());
    }
    if !subs.exists() {
        bail!("subtitle input does not exist: {}", subs.display());
    }

    let loaded = load_config(config_path)?;
    let style = resolve_style(
        &loaded.config.translate.line_order,
        &loaded.config.burnin.font,
        loaded.config.burnin.size,
        loaded.config.burnin.line_spacing,
        &loaded.config.burnin.styles,
        font,
        size,
    );

    let output = output.unwrap_or_else(|| default_output_path(&video));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create output directory for burnin {}",
                output.display()
            )
        })?;
    }

    burn_in_subtitles(&loaded.paths.runtime_home, &video, &subs, &output, &style)?;

    Ok(BurninOutput {
        video,
        subs,
        output,
        font: style.font_label(),
        size: style.size.unwrap_or(0),
        line_spacing: style.line_spacing.unwrap_or(0),
        status: "rendered",
    })
}

fn default_output_path(video: &Path) -> PathBuf {
    let stem = video
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("video");
    let extension = video
        .extension()
        .and_then(|item| item.to_str())
        .unwrap_or("mp4");
    video.with_file_name(format!("{stem}.burned.{extension}"))
}

fn resolve_style(
    config_line_order: &[String],
    config_font: &str,
    config_size: u32,
    config_line_spacing: u32,
    config_styles: &HashMap<String, crate::config::BurninLineStyleConfig>,
    cli_font: Option<String>,
    cli_size: Option<u32>,
) -> BurninStyle {
    let font = cli_font.or_else(|| normalize_font(config_font));
    let size = match cli_size.unwrap_or(config_size) {
        0 => None,
        value => Some(value),
    };
    let line_spacing = match config_line_spacing {
        0 => None,
        value => Some(value),
    };
    let line_styles = config_styles
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                LineStyle {
                    font: value.font.clone(),
                    size: value.size,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    BurninStyle {
        font,
        size,
        line_spacing,
        line_order: config_line_order.to_vec(),
        line_styles,
    }
}

fn normalize_font(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        None
    } else {
        Some(trimmed.to_string())
    }
}
