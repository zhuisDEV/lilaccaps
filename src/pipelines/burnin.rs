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
    pub colour: String,
    pub size: u32,
    pub line_spacing: u32,
    pub renderer: &'static str,
    pub renderer_reason: String,
    pub status: &'static str,
}

pub fn run(
    video: PathBuf,
    config_path: Option<PathBuf>,
    subs: PathBuf,
    output: Option<PathBuf>,
    font: Option<String>,
    colour: Option<String>,
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
        loaded.config.burnin.advanced_styling,
        &loaded.config.burnin.font,
        &loaded.config.burnin.colour,
        loaded.config.burnin.size,
        loaded.config.burnin.line_spacing,
        &loaded.config.burnin.styles,
        font,
        colour,
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

    let renderer = burn_in_subtitles(&loaded.paths.runtime_home, &video, &subs, &output, &style)?;

    Ok(BurninOutput {
        video,
        subs,
        output,
        font: style.font_label(),
        colour: style.colour_label(),
        size: style.size.unwrap_or(0),
        line_spacing: style.line_spacing.unwrap_or(0),
        renderer: renderer.renderer,
        renderer_reason: if renderer.reasons.is_empty() {
            "none".to_string()
        } else {
            renderer.reasons.join(",")
        },
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

#[allow(clippy::too_many_arguments)]
fn resolve_style(
    config_line_order: &[String],
    config_advanced_styling: bool,
    config_font: &str,
    config_colour: &str,
    config_size: u32,
    config_line_spacing: u32,
    config_styles: &HashMap<String, crate::config::BurninLineStyleConfig>,
    cli_font: Option<String>,
    cli_colour: Option<String>,
    cli_size: Option<u32>,
) -> BurninStyle {
    let font = cli_font.or_else(|| normalize_font(config_font));
    let colour = cli_colour.or_else(|| {
        if config_advanced_styling {
            normalize_colour(config_colour)
        } else {
            None
        }
    });
    let size = match cli_size.unwrap_or(config_size) {
        0 => None,
        value => Some(value),
    };
    let line_spacing = if config_advanced_styling {
        match config_line_spacing {
            0 => None,
            value => Some(value),
        }
    } else {
        None
    };
    let line_styles = if config_advanced_styling {
        config_styles
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    LineStyle {
                        font: value.font.as_deref().and_then(normalize_font),
                        colour: value.colour.as_deref().and_then(normalize_colour),
                        size: value.size,
                    },
                )
            })
            .collect::<HashMap<_, _>>()
    } else {
        HashMap::new()
    };

    BurninStyle {
        font,
        colour,
        size,
        line_spacing,
        line_order: if config_advanced_styling {
            config_line_order.to_vec()
        } else {
            Vec::new()
        },
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

fn normalize_colour(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::resolve_style;
    use crate::config::BurninLineStyleConfig;

    #[test]
    fn disabling_advanced_styling_clears_config_advanced_fields() {
        let style = resolve_style(
            &["source".to_string(), "en".to_string()],
            false,
            "auto",
            "#ffd54f",
            0,
            1,
            &HashMap::from([(
                "source".to_string(),
                BurninLineStyleConfig {
                    font: Some("auto".to_string()),
                    colour: Some("#ffd54f".to_string()),
                    size: Some(30),
                },
            )]),
            None,
            None,
            None,
        );

        assert!(style.colour.is_none());
        assert!(style.line_spacing.is_none());
        assert!(style.line_order.is_empty());
        assert!(style.line_styles.is_empty());
    }

    #[test]
    fn cli_colour_overrides_disabled_advanced_styling() {
        let style = resolve_style(
            &["source".to_string(), "en".to_string()],
            false,
            "auto",
            "auto",
            0,
            1,
            &HashMap::from([(
                "source".to_string(),
                BurninLineStyleConfig {
                    font: Some("auto".to_string()),
                    colour: Some("#ffd54f".to_string()),
                    size: Some(30),
                },
            )]),
            None,
            Some("#00ff00".to_string()),
            None,
        );

        assert_eq!(style.colour.as_deref(), Some("#00ff00"));
        assert!(style.line_spacing.is_none());
        assert!(style.line_order.is_empty());
        assert!(style.line_styles.is_empty());
    }
}
