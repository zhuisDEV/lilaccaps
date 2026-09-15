use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::fonts::resolve_font_path;
use crate::media::{
    ass_colour, ensure_ffmpeg_available, ffmpeg_supports_filter, subtitles_filter, video_size,
};
use crate::runtime::{MAGICK_DEPENDENCY, ScopedTempPath, ensure_dependency, tmp_dir};
use crate::subtitles::{SrtCue, parse_srt_file};
use crate::watermark::{
    WatermarkSource, WatermarkStyle, convert_image_watermark, image_needs_conversion,
    normalized_opacity, render_text_watermark_image, text_filter,
};

#[derive(Debug, Clone)]
pub struct BurninStyle {
    pub font: Option<String>,
    pub colour: Option<String>,
    pub size: Option<u32>,
    pub line_spacing: Option<u32>,
    pub outline: OutlineStyle,
    pub line_order: Vec<String>,
    pub line_styles: HashMap<String, LineStyle>,
}

#[derive(Debug, Clone, Default)]
pub struct OutlineStyle {
    pub enabled: bool,
    pub colour: Option<String>,
    pub width: u32,
}

#[derive(Debug, Clone)]
pub struct BurninRendererReport {
    pub renderer: &'static str,
    pub reasons: Vec<&'static str>,
}

impl BurninStyle {
    pub fn font_label(&self) -> String {
        self.font.clone().unwrap_or_else(|| "auto".to_string())
    }

    pub fn colour_label(&self) -> String {
        self.colour.clone().unwrap_or_else(|| "auto".to_string())
    }

    fn has_line_overrides(&self) -> bool {
        !self.line_order.is_empty() && !self.line_styles.is_empty()
    }

    fn uses_overlay_renderer(&self) -> bool {
        self.has_line_overrides()
            || self.line_spacing.is_some()
            || self.colour.is_some()
            || self.outline.requires_overlay_renderer()
    }
}

impl OutlineStyle {
    pub fn is_active(&self) -> bool {
        self.enabled && self.width > 0
    }

    pub fn active_width(&self) -> u32 {
        if self.is_active() { self.width } else { 0 }
    }

    pub fn colour_label(&self) -> String {
        if self.is_active() {
            self.colour.clone().unwrap_or_else(|| "black".to_string())
        } else {
            "none".to_string()
        }
    }

    fn requires_overlay_renderer(&self) -> bool {
        self.is_active()
            && self
                .colour
                .as_deref()
                .is_some_and(|colour| ass_colour(colour).is_none())
    }
}

#[derive(Debug, Clone, Default)]
pub struct LineStyle {
    pub font: Option<String>,
    pub colour: Option<String>,
    pub size: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct TextLayerSpec<'a> {
    font_path: &'a str,
    fill_colour: &'a str,
    stroke_colour: &'a str,
    stroke_width: Option<u32>,
    point_size: u32,
    text_source: &'a str,
    wrap_width: u32,
}

pub fn burn_in_subtitles(
    runtime_home: &Path,
    video: &Path,
    subs: &Path,
    output: &Path,
    style: &BurninStyle,
) -> Result<BurninRendererReport> {
    burn_in_subtitles_with_watermark(runtime_home, video, subs, output, style, None)
}

/// Compose captions and an optional watermark before a single video encode.
/// Audio is copied from the original input; callers publish the candidate after verification.
pub fn burn_in_subtitles_with_watermark(
    runtime_home: &Path,
    video: &Path,
    subs: &Path,
    output: &Path,
    style: &BurninStyle,
    watermark: Option<(&WatermarkSource, &WatermarkStyle)>,
) -> Result<BurninRendererReport> {
    ensure_ffmpeg_available()?;
    for input in [video, subs] {
        if crate::runtime::paths_refer_to_same_file(input, output)? {
            bail!(
                "render output must be different from input: {}",
                input.display()
            );
        }
    }
    if let Some((WatermarkSource::Image(image), _)) = watermark
        && crate::runtime::paths_refer_to_same_file(image, output)?
    {
        bail!(
            "render output must be different from watermark image: {}",
            image.display()
        );
    }
    let native_subtitles = !style.uses_overlay_renderer() && ffmpeg_supports_filter("subtitles")?;
    let work_dir = ScopedTempPath::directory(&tmp_dir(runtime_home), "render")?;
    let overlays = if native_subtitles {
        Vec::new()
    } else {
        ensure_dependency(MAGICK_DEPENDENCY)?;
        let cues = parse_srt_file(subs)?;
        if cues.is_empty() {
            bail!("subtitle file contained no cues: {}", subs.display());
        }
        let (width, height) = video_size(video)?;
        render_overlay_images(work_dir.path(), width, height, &cues, style)?
    };
    let native_filter = native_subtitles.then(|| {
        subtitles_filter(
            subs,
            style.font.as_deref(),
            style.size,
            style.outline.colour.as_deref(),
            Some(style.outline.active_width()),
        )
    });
    let mut prepared = watermark
        .map(|(source, style)| prepare_watermark(source, style, work_dir.path(), false))
        .transpose()?;
    let mut reasons = if native_subtitles {
        Vec::new()
    } else {
        overlay_renderer_reasons(style)
    };
    if !native_subtitles && reasons.is_empty() {
        reasons.push("ffmpeg_subtitles_filter_unavailable");
    }
    if matches!(
        prepared,
        Some(PreparedWatermark::Image {
            from_text: true,
            ..
        })
    ) {
        reasons.push("watermark_drawtext_unavailable");
    }
    let rendered = render_composite(
        video,
        output,
        native_filter.as_deref(),
        &overlays,
        prepared.as_ref(),
    );
    if let Err(error) = rendered {
        if matches!(prepared, Some(PreparedWatermark::Text { .. })) {
            let (source, style) = watermark.expect("prepared text came from a watermark");
            prepared = Some(prepare_watermark(source, style, work_dir.path(), true)?);
            render_composite(
                video,
                output,
                native_filter.as_deref(),
                &overlays,
                prepared.as_ref(),
            )
            .with_context(|| {
                format!("native text watermark failed ({error}); image fallback also failed")
            })?;
            reasons.push("watermark_drawtext_failed");
        } else {
            return Err(error);
        }
    }
    Ok(BurninRendererReport {
        renderer: match (native_subtitles, watermark.is_some()) {
            (true, false) => "ffmpeg-subtitles",
            (false, false) => "overlay-fallback",
            (true, true) => "ffmpeg-subtitles+watermark",
            (false, true) => "overlay-fallback+watermark",
        },
        reasons,
    })
}

enum PreparedWatermark<'a> {
    Text {
        text: &'a str,
        style: &'a WatermarkStyle,
    },
    Image {
        path: PathBuf,
        style: WatermarkStyle,
        from_text: bool,
        _temporary: Option<ScopedTempPath>,
    },
}

fn prepare_watermark<'a>(
    source: &'a WatermarkSource,
    style: &'a WatermarkStyle,
    work_dir: &Path,
    force_text_overlay: bool,
) -> Result<PreparedWatermark<'a>> {
    normalized_opacity(style.opacity)?;
    match source {
        WatermarkSource::Text(text) => {
            if text.trim().is_empty() {
                bail!("watermark text must not be empty");
            }
            if style.font.is_some() {
                resolve_font_path(style.font.as_deref(), text)?;
            }
            if !force_text_overlay && ffmpeg_supports_filter("drawtext")? {
                return Ok(PreparedWatermark::Text { text, style });
            }
            let image = ScopedTempPath::file(work_dir, "watermark-text", Some("png"));
            render_text_watermark_image(text, style, image.path())?;
            let mut image_style = style.clone();
            image_style.size = 0;
            Ok(PreparedWatermark::Image {
                path: image.path().to_path_buf(),
                style: image_style,
                from_text: true,
                _temporary: Some(image),
            })
        }
        WatermarkSource::Image(path) => {
            if !path.is_file() {
                bail!("watermark image does not exist: {}", path.display());
            }
            if image_needs_conversion(path) {
                let image = ScopedTempPath::file(work_dir, "watermark-image", Some("png"));
                convert_image_watermark(path, image.path(), style)?;
                Ok(PreparedWatermark::Image {
                    path: image.path().to_path_buf(),
                    style: style.clone(),
                    from_text: false,
                    _temporary: Some(image),
                })
            } else {
                Ok(PreparedWatermark::Image {
                    path: path.clone(),
                    style: style.clone(),
                    from_text: false,
                    _temporary: None,
                })
            }
        }
    }
}

fn render_overlay_images(
    work_dir: &Path,
    width: u32,
    height: u32,
    cues: &[SrtCue],
    style: &BurninStyle,
) -> Result<Vec<(SrtCue, PathBuf)>> {
    let mut overlays = Vec::with_capacity(cues.len());

    for (sequence, cue) in cues.iter().enumerate() {
        let image_path = work_dir.join(format!("cue-{sequence:04}-{}.png", cue.index));
        let image_target = format!("PNG32:{}", image_path.display());
        let status = render_overlay_image(work_dir, width, height, cue, style, &image_target)
            .with_context(|| format!("failed to start ImageMagick for cue {}", cue.index))?;

        if !status.success() {
            bail!(
                "ImageMagick failed while rendering subtitle cue {}",
                cue.index
            );
        }

        overlays.push((cue.clone(), image_path));
    }

    Ok(overlays)
}

fn render_overlay_image(
    work_dir: &Path,
    width: u32,
    height: u32,
    cue: &SrtCue,
    style: &BurninStyle,
    image_target: &str,
) -> Result<std::process::ExitStatus> {
    let lines = cue.text.lines().collect::<Vec<_>>();
    let mut command = Command::new("magick");
    let mut caption_files = Vec::new();
    let default_point_size = style.size.unwrap_or_else(|| point_size_for_height(height));
    let wrap_width = subtitle_wrap_width(width);

    if style.has_line_overrides() && lines.len() > 1 {
        for (index, line) in lines.iter().enumerate() {
            let text_source = caption_text_source(work_dir, line, &mut caption_files)?;
            let line_style = line_style_for_index(style, index);
            let font_path =
                resolve_font_path(line_style.font.as_deref().or(style.font.as_deref()), line)?;
            let font_path = font_path.to_string_lossy();
            let fill_colour = line_style
                .colour
                .as_deref()
                .or(style.colour.as_deref())
                .unwrap_or("white");
            let point_size = line_style.size.or(style.size).unwrap_or(default_point_size);
            let vertical_padding = style
                .line_spacing
                .unwrap_or_else(|| multiline_line_padding(point_size));
            append_text_with_shadow(
                &mut command,
                &font_path,
                fill_colour,
                point_size,
                &text_source,
                wrap_width,
                style,
            )
            .arg("-bordercolor")
            .arg("none")
            .arg("-border")
            .arg(format!("0x{vertical_padding}"))
            .arg(")");
        }

        command
            .arg("-background")
            .arg("none")
            .arg("-gravity")
            .arg("center")
            .arg("-append");
    } else {
        let text_source = caption_text_source(work_dir, &cue.text, &mut caption_files)?;
        let font_path = resolve_font_path(style.font.as_deref(), &cue.text)?;
        let font_path = font_path.to_string_lossy();
        let fill_colour = style.colour.as_deref().unwrap_or("white");
        append_text_with_shadow(
            &mut command,
            &font_path,
            fill_colour,
            default_point_size,
            &text_source,
            wrap_width,
            style,
        )
        .arg(")");
    }

    Ok(command
        .arg("-gravity")
        .arg("south")
        .arg("-background")
        .arg("none")
        .arg("-extent")
        .arg(format!("{width}x{height}"))
        .arg("-gravity")
        .arg("south")
        .arg("-splice")
        .arg("0x40")
        .arg(image_target)
        .status()?)
}

fn append_text_with_shadow<'a>(
    command: &'a mut Command,
    font_path: &str,
    fill_colour: &str,
    point_size: u32,
    text_source: &str,
    wrap_width: u32,
    style: &BurninStyle,
) -> &'a mut Command {
    command.arg("(");
    append_text_label(
        command,
        font_path,
        fill_colour,
        point_size,
        text_source,
        wrap_width,
        &style.outline,
    );
    command
        .arg("(")
        .arg("+clone")
        .arg("-background")
        .arg("black")
        .arg("-shadow")
        .arg("100x1+0+0")
        .arg(")")
        .arg("+swap")
        .arg("-background")
        .arg("none")
        .arg("-layers")
        .arg("merge")
        .arg("+repage")
}

fn append_text_label(
    command: &mut Command,
    font_path: &str,
    fill_colour: &str,
    point_size: u32,
    text_source: &str,
    wrap_width: u32,
    outline: &OutlineStyle,
) {
    if outline.is_active() {
        command.arg("(");
        append_label_layer(
            command,
            TextLayerSpec {
                font_path,
                fill_colour: "none",
                stroke_colour: outline.colour.as_deref().unwrap_or("black"),
                stroke_width: Some(outline.width),
                point_size,
                text_source,
                wrap_width,
            },
        );
        command.arg(")");
    }

    command.arg("(");
    append_label_layer(
        command,
        TextLayerSpec {
            font_path,
            fill_colour,
            stroke_colour: "none",
            stroke_width: None,
            point_size,
            text_source,
            wrap_width,
        },
    );
    command.arg(")");

    if outline.is_active() {
        command
            .arg("-background")
            .arg("none")
            .arg("-layers")
            .arg("merge")
            .arg("+repage");
    }
}

fn append_label_layer(command: &mut Command, spec: TextLayerSpec<'_>) {
    for arg in label_layer_args(spec) {
        command.arg(arg);
    }
}

fn label_layer_args(spec: TextLayerSpec<'_>) -> Vec<String> {
    let mut args = vec![
        "-background".to_string(),
        "none".to_string(),
        "-font".to_string(),
        spec.font_path.to_string(),
        "-fill".to_string(),
        spec.fill_colour.to_string(),
        "-stroke".to_string(),
        spec.stroke_colour.to_string(),
    ];

    if let Some(stroke_width) = spec.stroke_width {
        args.push("-strokewidth".to_string());
        args.push(stroke_width.to_string());
    }

    args.extend([
        "-pointsize".to_string(),
        spec.point_size.to_string(),
        "-size".to_string(),
        format!("{}x", spec.wrap_width),
        spec.text_source.to_string(),
    ]);
    args
}

fn caption_text_source(
    work_dir: &Path,
    text: &str,
    files: &mut Vec<ScopedTempPath>,
) -> Result<String> {
    let file = ScopedTempPath::file(work_dir, "caption-text", Some("txt"));
    fs::write(file.path(), text).with_context(|| {
        format!(
            "failed to write temporary caption text {}",
            file.path().display()
        )
    })?;
    let source = format!("caption:@{}", file.path().display());
    files.push(file);
    Ok(source)
}

fn render_composite(
    video: &Path,
    output: &Path,
    native_subtitles: Option<&str>,
    overlays: &[(SrtCue, PathBuf)],
    watermark: Option<&PreparedWatermark<'_>>,
) -> Result<()> {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(video);

    // Keep the simple native path equivalent to the standalone burn-in command.
    if let Some(subtitles) = native_subtitles
        && !matches!(watermark, Some(PreparedWatermark::Image { .. }))
    {
        let filter = match watermark {
            Some(PreparedWatermark::Text { text, style }) => {
                format!("{subtitles},{}", text_filter(text, style))
            }
            _ => subtitles.to_string(),
        };
        command.args(["-vf", &filter, "-map", "0:v:0"]);
    } else {
        let mut graph = Vec::new();
        let mut previous = "0:v:0".to_string();
        if let Some(subtitles) = native_subtitles {
            graph.push(format!("[{previous}]{subtitles}[captioned]"));
            previous = "captioned".to_string();
        }
        for (index, (cue, image)) in overlays.iter().enumerate() {
            command.arg("-i").arg(image);
            let input = index + 1;
            let start = cue.start_ms as f64 / 1000.0;
            let end = cue.end_ms as f64 / 1000.0;
            let next = format!("caption{index}");
            graph.push(format!("[{input}:v]format=rgba[ov{index}];[{previous}][ov{index}]overlay=0:0:enable='between(t,{start:.3},{end:.3})'[{next}]"));
            previous = next;
        }
        match watermark {
            Some(PreparedWatermark::Text { text, style }) => {
                graph.push(format!(
                    "[{previous}]{}[watermarked]",
                    text_filter(text, style)
                ));
                previous = "watermarked".to_string();
            }
            Some(PreparedWatermark::Image { path, style, .. }) => {
                command.arg("-i").arg(path);
                let input = overlays.len() + 1;
                let resize = if style.size == 0 {
                    String::new()
                } else {
                    format!("scale={}:-1:flags=lanczos,", style.size)
                };
                let (x, y) = style.position.overlay_xy(style.margin);
                graph.push(format!("[{input}:v]{resize}format=rgba,colorchannelmixer=aa={:.3}[wm];[{previous}][wm]overlay=x={x}:y={y}:format=auto[watermarked]", style.opacity));
                previous = "watermarked".to_string();
            }
            None => {}
        }
        if graph.is_empty() {
            bail!("no subtitle or watermark filters were generated");
        }
        command
            .arg("-filter_complex")
            .arg(graph.join(";"))
            .arg("-map")
            .arg(format!("[{previous}]"));
    }
    command
        .args([
            "-map",
            "0:a?",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "copy",
            "-movflags",
            "+faststart",
        ])
        .arg(output);
    let status = command
        .status()
        .with_context(|| format!("failed to start ffmpeg render for {}", video.display()))?;
    if !status.success() {
        bail!(
            "ffmpeg failed while compositing captions and watermark for {}",
            video.display()
        );
    }
    Ok(())
}

fn point_size_for_height(height: u32) -> u32 {
    let candidate = height / 17;
    candidate.max(28)
}

fn subtitle_wrap_width(width: u32) -> u32 {
    (width.saturating_mul(9) / 10).max(1)
}

fn multiline_line_padding(point_size: u32) -> u32 {
    (point_size / 24).clamp(1, 2)
}

fn overlay_renderer_reasons(style: &BurninStyle) -> Vec<&'static str> {
    let mut reasons = Vec::new();

    if style.has_line_overrides() {
        reasons.push("per_line_styles");
    }

    if style.line_spacing.is_some() {
        reasons.push("line_spacing");
    }

    if style.colour.is_some() {
        reasons.push("colour");
    }

    if style.outline.requires_overlay_renderer() {
        reasons.push("outline_colour");
    }

    reasons
}

fn line_style_for_index(style: &BurninStyle, index: usize) -> LineStyle {
    let role = style
        .line_order
        .get(index)
        .map(String::as_str)
        .unwrap_or("");
    style.line_styles.get(role).cloned().unwrap_or_default()
}

#[cfg(test)]
#[path = "../tests/rendering/combined.rs"]
mod combined_tests;

#[cfg(test)]
mod tests {
    use super::{
        BurninStyle, LineStyle, OutlineStyle, TextLayerSpec, label_layer_args,
        line_style_for_index, multiline_line_padding, overlay_renderer_reasons,
        subtitle_wrap_width,
    };
    use std::collections::HashMap;

    #[test]
    fn resolves_line_style_by_ordered_role() {
        let mut line_styles = HashMap::new();
        line_styles.insert(
            "en".to_string(),
            LineStyle {
                font: Some("Arial".to_string()),
                colour: Some("#ffd54f".to_string()),
                size: Some(30),
            },
        );
        let style = BurninStyle {
            font: None,
            colour: None,
            size: None,
            line_spacing: None,
            outline: OutlineStyle::default(),
            line_order: vec!["source".to_string(), "en".to_string()],
            line_styles,
        };

        let line_style = line_style_for_index(&style, 1);
        assert_eq!(line_style.font.as_deref(), Some("Arial"));
        assert_eq!(line_style.size, Some(30));
    }

    #[test]
    fn multiline_padding_stays_tight() {
        assert_eq!(multiline_line_padding(30), 1);
        assert_eq!(multiline_line_padding(42), 1);
        assert_eq!(multiline_line_padding(60), 2);
    }

    #[test]
    fn overlay_caption_width_leaves_horizontal_margin() {
        assert_eq!(subtitle_wrap_width(1920), 1728);
        assert_eq!(subtitle_wrap_width(1280), 1152);
        assert_eq!(subtitle_wrap_width(0), 1);
    }

    #[test]
    fn overlay_text_layers_use_bounded_caption_images() {
        let args = label_layer_args(TextLayerSpec {
            font_path: "/Library/Fonts/Arial.ttf",
            fill_colour: "white",
            stroke_colour: "black",
            stroke_width: Some(2),
            point_size: 42,
            text_source: "caption:@/tmp/caption.txt",
            wrap_width: 1152,
        });

        assert!(args.windows(2).any(|items| items == ["-size", "1152x"]));
        assert!(args.iter().any(|arg| arg == "caption:@/tmp/caption.txt"));
        assert!(!args.iter().any(|arg| arg.starts_with("label:")));
    }

    #[test]
    fn line_spacing_forces_overlay_renderer() {
        let style = BurninStyle {
            font: None,
            colour: None,
            size: None,
            line_spacing: Some(3),
            outline: OutlineStyle::default(),
            line_order: Vec::new(),
            line_styles: HashMap::new(),
        };

        assert!(style.uses_overlay_renderer());
    }

    #[test]
    fn explicit_colour_forces_overlay_renderer() {
        let style = BurninStyle {
            font: None,
            colour: Some("#ffd54f".to_string()),
            size: None,
            line_spacing: None,
            outline: OutlineStyle::default(),
            line_order: Vec::new(),
            line_styles: HashMap::new(),
        };

        assert!(style.uses_overlay_renderer());
    }

    #[test]
    fn overlay_renderer_reasons_report_active_features() {
        let style = BurninStyle {
            font: None,
            colour: Some("#ffd54f".to_string()),
            size: None,
            line_spacing: Some(1),
            outline: OutlineStyle::default(),
            line_order: vec!["source".to_string()],
            line_styles: HashMap::from([("source".to_string(), LineStyle::default())]),
        };

        let reasons = overlay_renderer_reasons(&style);
        assert_eq!(reasons, vec!["per_line_styles", "line_spacing", "colour"]);
    }

    #[test]
    fn ass_supported_outline_stays_on_primary_renderer() {
        let style = BurninStyle {
            font: None,
            colour: None,
            size: None,
            line_spacing: None,
            outline: OutlineStyle {
                enabled: true,
                colour: Some("black".to_string()),
                width: 2,
            },
            line_order: Vec::new(),
            line_styles: HashMap::new(),
        };

        assert!(!style.uses_overlay_renderer());
        assert!(overlay_renderer_reasons(&style).is_empty());
    }

    #[test]
    fn ass_unsupported_outline_colour_forces_overlay_renderer() {
        let style = BurninStyle {
            font: None,
            colour: None,
            size: None,
            line_spacing: None,
            outline: OutlineStyle {
                enabled: true,
                colour: Some("rgba(0,0,0,0.5)".to_string()),
                width: 2,
            },
            line_order: Vec::new(),
            line_styles: HashMap::new(),
        };

        assert!(style.uses_overlay_renderer());
        assert_eq!(overlay_renderer_reasons(&style), vec!["outline_colour"]);
    }
}
