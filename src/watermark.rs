use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;

use crate::media::{ensure_ffmpeg_available, ffmpeg_supports_filter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WatermarkPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

#[derive(Debug, Clone)]
pub enum WatermarkSource {
    Text(String),
    Image(PathBuf),
}

#[derive(Debug, Clone)]
pub struct WatermarkStyle {
    pub position: WatermarkPosition,
    pub opacity: f32,
    pub size: u32,
    pub margin: u32,
    pub colour: String,
    pub font: Option<String>,
}

impl WatermarkPosition {
    pub fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
            Self::Center => "center",
        }
    }

    fn drawtext_xy(self, margin: u32) -> (String, String) {
        match self {
            Self::TopLeft => (margin.to_string(), margin.to_string()),
            Self::TopRight => (format!("w-text_w-{margin}"), margin.to_string()),
            Self::BottomLeft => (margin.to_string(), format!("h-text_h-{margin}")),
            Self::BottomRight => (format!("w-text_w-{margin}"), format!("h-text_h-{margin}")),
            Self::Center => ("(w-text_w)/2".to_string(), "(h-text_h)/2".to_string()),
        }
    }

    fn overlay_xy(self, margin: u32) -> (String, String) {
        match self {
            Self::TopLeft => (margin.to_string(), margin.to_string()),
            Self::TopRight => (format!("main_w-overlay_w-{margin}"), margin.to_string()),
            Self::BottomLeft => (margin.to_string(), format!("main_h-overlay_h-{margin}")),
            Self::BottomRight => (
                format!("main_w-overlay_w-{margin}"),
                format!("main_h-overlay_h-{margin}"),
            ),
            Self::Center => (
                "(main_w-overlay_w)/2".to_string(),
                "(main_h-overlay_h)/2".to_string(),
            ),
        }
    }
}

impl WatermarkSource {
    pub fn label(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Image(path) => path.display().to_string(),
        }
    }
}

pub fn apply_watermark(
    video: &Path,
    output: &Path,
    source: &WatermarkSource,
    style: &WatermarkStyle,
) -> Result<()> {
    ensure_ffmpeg_available()?;

    match source {
        WatermarkSource::Text(text) => apply_text_watermark(video, output, text, style),
        WatermarkSource::Image(image) => apply_image_watermark(video, output, image, style),
    }
}

pub fn normalized_opacity(opacity: f32) -> Result<f32> {
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        bail!("watermark opacity must be between 0.0 and 1.0");
    }

    Ok(opacity)
}

fn apply_text_watermark(
    video: &Path,
    output: &Path,
    text: &str,
    style: &WatermarkStyle,
) -> Result<()> {
    ensure_filter_available("drawtext")?;
    let filter = text_filter(text, style);
    let status = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(video)
        .arg("-vf")
        .arg(filter)
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("0:a?")
        .arg("-c:v")
        .arg("libx264")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-c:a")
        .arg("copy")
        .arg(output)
        .status()
        .with_context(|| format!("failed to start ffmpeg watermark for {}", video.display()))?;

    if !status.success() {
        bail!(
            "ffmpeg failed while applying text watermark to {}",
            video.display()
        );
    }

    Ok(())
}

fn apply_image_watermark(
    video: &Path,
    output: &Path,
    image: &Path,
    style: &WatermarkStyle,
) -> Result<()> {
    ensure_filter_available("overlay")?;
    let filter = image_filter(style);
    let status = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(video)
        .arg("-i")
        .arg(image)
        .arg("-filter_complex")
        .arg(filter)
        .arg("-map")
        .arg("[v]")
        .arg("-map")
        .arg("0:a?")
        .arg("-c:v")
        .arg("libx264")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-c:a")
        .arg("copy")
        .arg(output)
        .status()
        .with_context(|| format!("failed to start ffmpeg watermark for {}", video.display()))?;

    if !status.success() {
        bail!(
            "ffmpeg failed while applying image watermark to {}",
            video.display()
        );
    }

    Ok(())
}

pub fn text_filter(text: &str, style: &WatermarkStyle) -> String {
    let (x, y) = style.position.drawtext_xy(style.margin);
    let font_size = if style.size == 0 { 32 } else { style.size };
    let colour = match style.colour.trim() {
        "" => "white",
        colour => colour,
    };
    let colour = escape_drawtext_value(colour);
    let mut options = vec![
        format!("text='{}'", escape_drawtext_text(text)),
        "expansion=none".to_string(),
        format!("x={x}"),
        format!("y={y}"),
        format!("fontsize={font_size}"),
        format!("fontcolor={colour}@{:.3}", style.opacity),
        format!("shadowcolor=black@{:.3}", (style.opacity + 0.25).min(1.0)),
        "shadowx=2".to_string(),
        "shadowy=2".to_string(),
    ];

    if let Some(font) = style.font.as_deref().map(str::trim)
        && !font.is_empty()
    {
        options.push(format!("font='{}'", escape_drawtext_value(font)));
    }

    format!("drawtext={}", options.join(":"))
}

fn ensure_filter_available(name: &str) -> Result<()> {
    if !ffmpeg_supports_filter(name)? {
        bail!("ffmpeg filter `{name}` is required for watermark rendering");
    }

    Ok(())
}

pub fn image_filter(style: &WatermarkStyle) -> String {
    let (x, y) = style.position.overlay_xy(style.margin);
    let image_chain = if style.size == 0 {
        "format=rgba".to_string()
    } else {
        format!("scale={}:-1:flags=lanczos,format=rgba", style.size)
    };

    format!(
        "[1:v]{image_chain},colorchannelmixer=aa={:.3}[wm];[0:v][wm]overlay=x={x}:y={y}:format=auto[v]",
        style.opacity
    )
}

fn escape_drawtext_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace(':', "\\:")
}

fn escape_drawtext_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace(':', "\\:")
}

#[cfg(test)]
mod tests {
    use super::{WatermarkPosition, WatermarkStyle, image_filter, normalized_opacity, text_filter};

    fn style(position: WatermarkPosition) -> WatermarkStyle {
        WatermarkStyle {
            position,
            opacity: 0.4,
            size: 0,
            margin: 24,
            colour: "white".to_string(),
            font: None,
        }
    }

    #[test]
    fn validates_opacity_range() {
        assert!(normalized_opacity(0.0).is_ok());
        assert!(normalized_opacity(1.0).is_ok());
        assert!(normalized_opacity(-0.1).is_err());
        assert!(normalized_opacity(1.1).is_err());
        assert!(normalized_opacity(f32::NAN).is_err());
    }

    #[test]
    fn text_filter_escapes_text_and_uses_bottom_right_default_shape() {
        let filter = text_filter("lilac: it's here", &style(WatermarkPosition::BottomRight));

        assert!(filter.contains("text='lilac\\: it\\'s here'"));
        assert!(filter.contains("x=w-text_w-24"));
        assert!(filter.contains("y=h-text_h-24"));
        assert!(filter.contains("fontcolor=white@0.400"));
    }

    #[test]
    fn text_filter_supports_font_colour_and_size() {
        let mut style = style(WatermarkPosition::TopLeft);
        style.font = Some("PingFang SC".to_string());
        style.colour = "#ffd54f".to_string();
        style.size = 42;

        let filter = text_filter("lilac", &style);

        assert!(filter.contains("fontsize=42"));
        assert!(filter.contains("fontcolor=#ffd54f@0.400"));
        assert!(filter.contains("font='PingFang SC'"));
    }

    #[test]
    fn blank_text_colour_falls_back_to_white() {
        let mut style = style(WatermarkPosition::TopLeft);
        style.colour = "  ".to_string();

        let filter = text_filter("lilac", &style);

        assert!(filter.contains("fontcolor=white@0.400"));
    }

    #[test]
    fn image_filter_scales_and_positions_overlay() {
        let mut style = style(WatermarkPosition::TopRight);
        style.size = 180;

        let filter = image_filter(&style);

        assert!(filter.contains("[1:v]scale=180:-1:flags=lanczos"));
        assert!(filter.contains("colorchannelmixer=aa=0.400"));
        assert!(filter.contains("overlay=x=main_w-overlay_w-24:y=24"));
    }
}
