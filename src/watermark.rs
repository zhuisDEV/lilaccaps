use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;

use crate::media::{ensure_ffmpeg_available, ffmpeg_supports_filter};
use crate::runtime::{MAGICK_DEPENDENCY, ScopedTempPath, ensure_dependency, parent_dir};

const ARIAL_FONT_CANDIDATES: [&str; 3] = [
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
];

const VERDANA_FONT_CANDIDATES: [&str; 2] = [
    "/System/Library/Fonts/Supplemental/Verdana.ttf",
    "/Library/Fonts/Verdana.ttf",
];

const HELVETICA_FONT_CANDIDATES: [&str; 2] = [
    "/System/Library/Fonts/Helvetica.ttc",
    "/System/Library/Fonts/HelveticaNeue.ttc",
];

const PINGFANG_FONT_CANDIDATES: [&str; 2] = [
    "/System/Library/AssetsV2/com_apple_MobileAsset_Font8/86ba2c91f017a3749571a82f2c6d890ac7ffb2fb.asset/AssetData/PingFang.ttc",
    "/System/Library/Fonts/PingFang.ttc",
];

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
    pub outline_colour: String,
    pub outline_width: u32,
}

#[derive(Debug, Clone)]
pub struct WatermarkRendererReport {
    pub renderer: &'static str,
    pub reasons: Vec<&'static str>,
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
) -> Result<WatermarkRendererReport> {
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
) -> Result<WatermarkRendererReport> {
    if !ffmpeg_supports_filter("drawtext")? {
        return apply_text_watermark_overlay_fallback(
            video,
            output,
            text,
            style,
            "drawtext_unavailable",
        );
    }

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
        return apply_text_watermark_overlay_fallback(
            video,
            output,
            text,
            style,
            "drawtext_failed",
        );
    }

    Ok(WatermarkRendererReport {
        renderer: "ffmpeg-drawtext",
        reasons: Vec::new(),
    })
}

fn apply_text_watermark_overlay_fallback(
    video: &Path,
    output: &Path,
    text: &str,
    style: &WatermarkStyle,
    reason: &'static str,
) -> Result<WatermarkRendererReport> {
    let rendered = ScopedTempPath::file(parent_dir(output), "watermark-text", Some("png"));
    render_text_watermark_image(text, style, rendered.path())?;
    let mut overlay_style = style.clone();
    overlay_style.size = 0;
    apply_image_watermark_with_reasons(
        video,
        output,
        rendered.path(),
        &overlay_style,
        "imagemagick-text-overlay",
        vec![reason],
    )
}

fn apply_image_watermark(
    video: &Path,
    output: &Path,
    image: &Path,
    style: &WatermarkStyle,
) -> Result<WatermarkRendererReport> {
    if image_needs_conversion(image) {
        let converted = ScopedTempPath::file(parent_dir(output), "watermark-image", Some("png"));
        convert_image_watermark(image, converted.path(), style)?;
        return apply_image_watermark_with_reasons(
            video,
            output,
            converted.path(),
            style,
            "image-overlay",
            vec!["image_converted"],
        );
    }

    apply_image_watermark_with_reasons(video, output, image, style, "image-overlay", Vec::new())
}

fn apply_image_watermark_with_reasons(
    video: &Path,
    output: &Path,
    image: &Path,
    style: &WatermarkStyle,
    renderer: &'static str,
    reasons: Vec<&'static str>,
) -> Result<WatermarkRendererReport> {
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

    Ok(WatermarkRendererReport { renderer, reasons })
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
        if let Some(font_path) = resolve_watermark_font(Some(font)) {
            options.push(format!("fontfile='{}'", escape_drawtext_value(&font_path)));
        } else {
            options.push(format!("font='{}'", escape_drawtext_value(font)));
        }
    }

    if style.outline_width > 0 {
        let outline_colour = match style.outline_colour.trim() {
            "" => "black",
            colour => colour,
        };
        options.push(format!("borderw={}", style.outline_width));
        options.push(format!(
            "bordercolor={}@{:.3}",
            escape_drawtext_value(outline_colour),
            style.opacity
        ));
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

fn render_text_watermark_image(text: &str, style: &WatermarkStyle, output: &Path) -> Result<()> {
    ensure_dependency(MAGICK_DEPENDENCY)?;

    let text_file = ScopedTempPath::file(parent_dir(output), "watermark-text", Some("txt"));
    fs::write(text_file.path(), text).with_context(|| {
        format!(
            "failed to write temporary watermark text {}",
            text_file.path().display()
        )
    })?;
    let text_source = format!("label:@{}", text_file.path().display());
    let output_target = format!("PNG32:{}", output.display());
    let status = Command::new("magick")
        .args(text_watermark_image_args(&text_source, style))
        .arg(output_target)
        .status()
        .with_context(|| {
            format!(
                "failed to start ImageMagick text watermark render for {}",
                output.display()
            )
        })?;

    if !status.success() {
        bail!("ImageMagick failed while rendering text watermark");
    }

    Ok(())
}

fn convert_image_watermark(image: &Path, output: &Path, style: &WatermarkStyle) -> Result<()> {
    ensure_dependency(MAGICK_DEPENDENCY)?;

    let output_target = format!("PNG32:{}", output.display());
    let mut command = Command::new("magick");
    if let Some(font) = resolve_watermark_font(style.font.as_deref()) {
        command.arg("-font").arg(font);
    }
    let status = command
        .arg(image)
        .arg(output_target)
        .status()
        .with_context(|| {
            format!(
                "failed to start ImageMagick watermark conversion for {}",
                image.display()
            )
        })?;

    if !status.success() {
        bail!(
            "ImageMagick failed while converting watermark image {}",
            image.display()
        );
    }

    Ok(())
}

fn text_watermark_image_args(text_source: &str, style: &WatermarkStyle) -> Vec<String> {
    let font_size = if style.size == 0 { 32 } else { style.size };
    let fill_colour = match style.colour.trim() {
        "" => "white",
        colour => colour,
    };
    let outline_colour = match style.outline_colour.trim() {
        "" => "black",
        colour => colour,
    };
    let font = resolve_watermark_font(style.font.as_deref())
        .unwrap_or_else(|| style.font.clone().unwrap_or_else(|| "Arial".to_string()));
    let border = style.outline_width.saturating_add(2);
    let mut args = Vec::new();

    if style.outline_width > 0 {
        args.push("(".to_string());
        args.push("(".to_string());
        append_label_layer_args(
            &mut args,
            &font,
            "none",
            outline_colour,
            Some(style.outline_width),
            font_size,
            text_source,
        );
        args.push(")".to_string());
        args.push("(".to_string());
        append_label_layer_args(
            &mut args,
            &font,
            fill_colour,
            "none",
            None,
            font_size,
            text_source,
        );
        args.push(")".to_string());
        args.push("-background".to_string());
        args.push("none".to_string());
        args.push("-layers".to_string());
        args.push("merge".to_string());
        args.push("+repage".to_string());
        args.push(")".to_string());
    } else {
        append_label_layer_args(
            &mut args,
            &font,
            fill_colour,
            "none",
            None,
            font_size,
            text_source,
        );
    }

    args.push("-bordercolor".to_string());
    args.push("none".to_string());
    args.push("-border".to_string());
    args.push(format!("{border}x{border}"));
    args
}

fn append_label_layer_args(
    args: &mut Vec<String>,
    font: &str,
    fill_colour: &str,
    stroke_colour: &str,
    stroke_width: Option<u32>,
    font_size: u32,
    text_source: &str,
) {
    args.push("-background".to_string());
    args.push("none".to_string());
    args.push("-font".to_string());
    args.push(font.to_string());
    args.push("-fill".to_string());
    args.push(fill_colour.to_string());
    args.push("-stroke".to_string());
    args.push(stroke_colour.to_string());

    if let Some(stroke_width) = stroke_width {
        args.push("-strokewidth".to_string());
        args.push(stroke_width.to_string());
    }

    args.push("-pointsize".to_string());
    args.push(font_size.to_string());
    args.push(text_source.to_string());
}

pub fn resolve_watermark_font(requested: Option<&str>) -> Option<String> {
    let requested = requested.map(str::trim).filter(|value| !value.is_empty());

    if let Some(requested) = requested {
        if Path::new(requested).exists() {
            return Some(requested.to_string());
        }

        if let Some(path) = named_font_candidates(requested)
            .iter()
            .copied()
            .find(|path| Path::new(path).exists())
        {
            return Some(path.to_string());
        }
    }

    default_watermark_font().map(str::to_string)
}

fn default_watermark_font() -> Option<&'static str> {
    VERDANA_FONT_CANDIDATES
        .iter()
        .chain(ARIAL_FONT_CANDIDATES.iter())
        .chain(HELVETICA_FONT_CANDIDATES.iter())
        .copied()
        .find(|path| Path::new(path).exists())
}

fn named_font_candidates(requested: &str) -> &'static [&'static str] {
    match normalize_font_name(requested).as_str() {
        "arial" => &ARIAL_FONT_CANDIDATES,
        "verdana" => &VERDANA_FONT_CANDIDATES,
        "helvetica" | "helveticaneue" => &HELVETICA_FONT_CANDIDATES,
        "pingfang" | "pingfangsc" => &PINGFANG_FONT_CANDIDATES,
        _ => &[],
    }
}

fn normalize_font_name(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn image_needs_conversion(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "svg" | "svgz"))
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
    use super::{
        WatermarkPosition, WatermarkStyle, image_filter, image_needs_conversion,
        named_font_candidates, normalized_opacity, text_filter, text_watermark_image_args,
    };
    use std::path::Path;

    fn style(position: WatermarkPosition) -> WatermarkStyle {
        WatermarkStyle {
            position,
            opacity: 0.4,
            size: 0,
            margin: 24,
            colour: "white".to_string(),
            font: None,
            outline_colour: "black".to_string(),
            outline_width: 0,
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
        assert!(filter.contains("fontfile=") || filter.contains("font='PingFang SC'"));
    }

    #[test]
    fn blank_text_colour_falls_back_to_white() {
        let mut style = style(WatermarkPosition::TopLeft);
        style.colour = "  ".to_string();

        let filter = text_filter("lilac", &style);

        assert!(filter.contains("fontcolor=white@0.400"));
    }

    #[test]
    fn text_filter_supports_outline() {
        let mut style = style(WatermarkPosition::TopLeft);
        style.outline_colour = "#000000".to_string();
        style.outline_width = 3;

        let filter = text_filter("lilac", &style);

        assert!(filter.contains("borderw=3"));
        assert!(filter.contains("bordercolor=#000000@0.400"));
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

    #[test]
    fn detects_svg_images_for_conversion() {
        assert!(image_needs_conversion(Path::new("/tmp/logo.svg")));
        assert!(image_needs_conversion(Path::new("/tmp/logo.SVGZ")));
        assert!(!image_needs_conversion(Path::new("/tmp/logo.png")));
    }

    #[test]
    fn maps_common_font_names_to_candidates() {
        assert!(!named_font_candidates("Arial").is_empty());
        assert!(!named_font_candidates("Verdana").is_empty());
        assert!(!named_font_candidates("PingFang SC").is_empty());
    }

    #[test]
    fn imagemagick_text_args_render_outline_and_fill_layers() {
        let mut style = style(WatermarkPosition::TopLeft);
        style.font = Some("Verdana".to_string());
        style.colour = "#E19CFF".to_string();
        style.outline_width = 2;

        let args = text_watermark_image_args("label:@/tmp/watermark.txt", &style);

        assert!(args.iter().any(|arg| arg == "-strokewidth"));
        assert!(args.iter().any(|arg| arg == "#E19CFF"));
        assert!(args.iter().any(|arg| arg == "label:@/tmp/watermark.txt"));
        assert!(args.iter().any(|arg| arg == "-layers"));
    }
}
