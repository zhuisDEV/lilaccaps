use std::fs;
use std::path::{Path, PathBuf};

use std::collections::HashMap;

use anyhow::{Context, Result, bail};

use crate::config::{
    BurninLineStyleConfig, BurninOutlineConfig, default_burnin_outline_width, load_config,
};
use crate::render::{BurninStyle, LineStyle, OutlineStyle, burn_in_subtitles};

#[derive(Debug, Clone)]
pub struct BurninOutput {
    pub video: PathBuf,
    pub subs: PathBuf,
    pub output: PathBuf,
    pub font: String,
    pub colour: String,
    pub size: u32,
    pub line_spacing: u32,
    pub outline_enabled: bool,
    pub outline_colour: String,
    pub outline_width: u32,
    pub renderer: &'static str,
    pub renderer_reason: String,
    pub status: &'static str,
}

#[derive(Debug, Clone)]
pub struct BurninRequest {
    pub video: PathBuf,
    pub config_path: Option<PathBuf>,
    pub subs: PathBuf,
    pub output: Option<PathBuf>,
    pub font: Option<String>,
    pub colour: Option<String>,
    pub size: Option<u32>,
    pub outline_enabled: Option<bool>,
    pub outline_colour: Option<String>,
    pub outline_width: Option<u32>,
}

pub fn run(request: BurninRequest) -> Result<BurninOutput> {
    let BurninRequest {
        video,
        config_path,
        subs,
        output,
        font,
        colour,
        size,
        outline_enabled,
        outline_colour,
        outline_width,
    } = request;

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
        &loaded.config.burnin.outline,
        &loaded.config.burnin.styles,
        font,
        colour,
        size,
        outline_enabled,
        outline_colour,
        outline_width,
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
        outline_enabled: style.outline.is_active(),
        outline_colour: style.outline.colour_label(),
        outline_width: style.outline.active_width(),
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
    config_outline: &BurninOutlineConfig,
    config_styles: &HashMap<String, BurninLineStyleConfig>,
    cli_font: Option<String>,
    cli_colour: Option<String>,
    cli_size: Option<u32>,
    cli_outline_enabled: Option<bool>,
    cli_outline_colour: Option<String>,
    cli_outline_width: Option<u32>,
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
        outline: resolve_outline(
            config_outline,
            cli_outline_enabled,
            cli_outline_colour,
            cli_outline_width,
        ),
        line_order: if config_advanced_styling {
            config_line_order.to_vec()
        } else {
            Vec::new()
        },
        line_styles,
    }
}

fn resolve_outline(
    config: &BurninOutlineConfig,
    cli_enabled: Option<bool>,
    cli_colour: Option<String>,
    cli_width: Option<u32>,
) -> OutlineStyle {
    let mut width = cli_width.unwrap_or(config.width);
    let cli_sets_outline = cli_colour.is_some() || cli_width.is_some_and(|value| value > 0);
    let enabled = cli_enabled.unwrap_or(if cli_sets_outline {
        true
    } else {
        config.enabled
    });
    if enabled && width == 0 && (cli_enabled == Some(true) || cli_sets_outline) {
        width = default_burnin_outline_width();
    }
    let colour = cli_colour
        .or_else(|| normalize_colour(&config.colour))
        .or_else(|| enabled.then(|| "black".to_string()));

    OutlineStyle {
        enabled,
        colour,
        width,
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
    use crate::config::{BurninLineStyleConfig, BurninOutlineConfig};

    #[test]
    fn disabling_advanced_styling_clears_config_advanced_fields() {
        let style = resolve_style(
            &["source".to_string(), "en".to_string()],
            false,
            "auto",
            "#ffd54f",
            0,
            1,
            &BurninOutlineConfig::default(),
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
            None,
            None,
            None,
        );

        assert!(style.colour.is_none());
        assert!(style.line_spacing.is_none());
        assert!(style.outline.is_active());
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
            &BurninOutlineConfig::default(),
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
            None,
            None,
            None,
        );

        assert_eq!(style.colour.as_deref(), Some("#00ff00"));
        assert!(style.line_spacing.is_none());
        assert!(style.outline.is_active());
        assert!(style.line_order.is_empty());
        assert!(style.line_styles.is_empty());
    }

    #[test]
    fn cli_outline_options_override_config_for_one_run() {
        let style = resolve_style(
            &[],
            true,
            "auto",
            "auto",
            0,
            0,
            &BurninOutlineConfig {
                enabled: false,
                colour: "black".to_string(),
                width: 2,
            },
            &HashMap::new(),
            None,
            None,
            None,
            None,
            Some("#111111".to_string()),
            Some(4),
        );

        assert!(style.outline.is_active());
        assert_eq!(style.outline.colour.as_deref(), Some("#111111"));
        assert_eq!(style.outline.width, 4);
    }

    #[test]
    fn cli_no_outline_disables_default_outline() {
        let style = resolve_style(
            &[],
            true,
            "auto",
            "auto",
            0,
            0,
            &BurninOutlineConfig::default(),
            &HashMap::new(),
            None,
            None,
            None,
            Some(false),
            None,
            None,
        );

        assert!(!style.outline.is_active());
    }

    #[test]
    fn cli_outline_re_enables_zero_width_config_with_default_width() {
        let style = resolve_style(
            &[],
            true,
            "auto",
            "auto",
            0,
            0,
            &BurninOutlineConfig {
                enabled: false,
                colour: "black".to_string(),
                width: 0,
            },
            &HashMap::new(),
            None,
            None,
            None,
            Some(true),
            None,
            None,
        );

        assert!(style.outline.is_active());
        assert_eq!(style.outline.width, 2);
    }
}
