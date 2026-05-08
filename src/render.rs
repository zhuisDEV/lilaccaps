use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::media::{ensure_ffmpeg_available, ffmpeg_supports_filter, subtitles_filter, video_size};
use crate::runtime::{MAGICK_DEPENDENCY, ensure_dependency, ensure_dir, tmp_dir};
use crate::subtitles::{SubtitleCue, parse_srt_file};

const CJK_FONT_CANDIDATES: [&str; 3] = [
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
];

const LATIN_FONT_CANDIDATES: [&str; 2] = [
    "/System/Library/Fonts/Helvetica.ttc",
    "/System/Library/Fonts/HelveticaNeue.ttc",
];

const PINGFANG_FONT_CANDIDATES: [&str; 2] = [
    "/System/Library/AssetsV2/com_apple_MobileAsset_Font8/86ba2c91f017a3749571a82f2c6d890ac7ffb2fb.asset/AssetData/PingFang.ttc",
    "/System/Library/Fonts/PingFang.ttc",
];

const ARIAL_FONT_CANDIDATES: [&str; 3] = [
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
];

const HIRAGINO_SANS_FONT_CANDIDATES: [&str; 2] = [
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
];

#[derive(Debug, Clone)]
pub struct BurninStyle {
    pub font: Option<String>,
    pub colour: Option<String>,
    pub size: Option<u32>,
    pub line_spacing: Option<u32>,
    pub line_order: Vec<String>,
    pub line_styles: HashMap<String, LineStyle>,
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
        self.has_line_overrides() || self.line_spacing.is_some() || self.colour.is_some()
    }
}

#[derive(Debug, Clone, Default)]
pub struct LineStyle {
    pub font: Option<String>,
    pub colour: Option<String>,
    pub size: Option<u32>,
}

pub fn burn_in_subtitles(
    runtime_home: &Path,
    video: &Path,
    subs: &Path,
    output: &Path,
    style: &BurninStyle,
) -> Result<BurninRendererReport> {
    ensure_ffmpeg_available()?;

    if !style.uses_overlay_renderer() && ffmpeg_supports_filter("subtitles")? {
        burn_in_with_subtitles_filter(video, subs, output, style)?;
        return Ok(BurninRendererReport {
            renderer: "ffmpeg-subtitles",
            reasons: Vec::new(),
        });
    }

    let reasons = overlay_renderer_reasons(style);
    burn_in_with_overlay_fallback(runtime_home, video, subs, output, style)?;
    Ok(BurninRendererReport {
        renderer: "overlay-fallback",
        reasons: if reasons.is_empty() {
            vec!["ffmpeg_subtitles_filter_unavailable"]
        } else {
            reasons
        },
    })
}

fn burn_in_with_subtitles_filter(
    video: &Path,
    subs: &Path,
    output: &Path,
    style: &BurninStyle,
) -> Result<()> {
    let filter = subtitles_filter(subs, style.font.as_deref(), style.size);
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
        .with_context(|| format!("failed to start ffmpeg burn-in for {}", video.display()))?;

    if !status.success() {
        bail!(
            "ffmpeg failed while burning subtitles into {}",
            video.display()
        );
    }

    Ok(())
}

fn burn_in_with_overlay_fallback(
    runtime_home: &Path,
    video: &Path,
    subs: &Path,
    output: &Path,
    style: &BurninStyle,
) -> Result<()> {
    ensure_dependency(MAGICK_DEPENDENCY)?;

    let cues = parse_srt_file(subs)?;
    if cues.is_empty() {
        bail!("subtitle file contained no cues: {}", subs.display());
    }

    let (width, height) = video_size(video)?;
    let work_dir = tmp_dir(runtime_home).join("burnin-overlays");
    ensure_dir(&work_dir)?;

    let overlays = render_overlay_images(&work_dir, width, height, &cues, style)?;
    burn_in_with_overlay_images(video, &overlays, output)
}

fn render_overlay_images(
    work_dir: &Path,
    width: u32,
    height: u32,
    cues: &[SubtitleCue],
    style: &BurninStyle,
) -> Result<Vec<(SubtitleCue, PathBuf)>> {
    let mut overlays = Vec::with_capacity(cues.len());

    for cue in cues {
        let image_path = work_dir.join(format!("cue-{:04}.png", cue.index));
        let image_target = format!("PNG32:{}", image_path.display());
        let status = render_overlay_image(width, height, cue, style, &image_target)
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
    width: u32,
    height: u32,
    cue: &SubtitleCue,
    style: &BurninStyle,
    image_target: &str,
) -> Result<std::process::ExitStatus> {
    let lines = cue.text.lines().collect::<Vec<_>>();
    let mut command = Command::new("magick");
    let default_point_size = style.size.unwrap_or_else(|| point_size_for_height(height));

    if style.has_line_overrides() && lines.len() > 1 {
        for (index, line) in lines.iter().enumerate() {
            let line_style = line_style_for_index(style, index, line);
            let font_path = line_style
                .font
                .as_deref()
                .or(style.font.as_deref())
                .map(|font| resolve_overlay_font(font, line))
                .unwrap_or_else(|| select_overlay_font(line).to_string());
            let fill_colour = line_style
                .colour
                .as_deref()
                .or(style.colour.as_deref())
                .unwrap_or("white");
            let point_size = line_style.size.or(style.size).unwrap_or(default_point_size);
            let vertical_padding = style
                .line_spacing
                .unwrap_or_else(|| multiline_line_padding(point_size));
            command
                .arg("(")
                .arg("(")
                .arg("-background")
                .arg("none")
                .arg("-font")
                .arg(&font_path)
                .arg("-fill")
                .arg(fill_colour)
                .arg("-pointsize")
                .arg(point_size.to_string())
                .arg(format!("label:{line}"))
                .arg(")")
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
        let font_path = style
            .font
            .as_deref()
            .map(|font| resolve_overlay_font(font, &cue.text))
            .unwrap_or_else(|| select_overlay_font(&cue.text).to_string());
        let fill_colour = style.colour.as_deref().unwrap_or("white");
        command
            .arg("(")
            .arg("-background")
            .arg("none")
            .arg("-font")
            .arg(&font_path)
            .arg("-fill")
            .arg(fill_colour)
            .arg("-pointsize")
            .arg(default_point_size.to_string())
            .arg(format!("label:{}", cue.text))
            .arg(")")
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
            .arg("+repage");
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

fn burn_in_with_overlay_images(
    video: &Path,
    overlays: &[(SubtitleCue, PathBuf)],
    output: &Path,
) -> Result<()> {
    if overlays.is_empty() {
        bail!("no subtitle overlays were generated");
    }

    let mut command = Command::new("ffmpeg");
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(video);

    for (_, image_path) in overlays {
        command.arg("-i").arg(image_path);
    }

    let mut filter_graph = String::new();
    let mut previous = "0:v".to_string();

    for (index, (cue, _)) in overlays.iter().enumerate() {
        let input = format!("{}:v", index + 1);
        let next = format!("v{}", index + 1);
        let start = cue.start_cs as f64 / 100.0;
        let end = cue.end_cs as f64 / 100.0;

        if !filter_graph.is_empty() {
            filter_graph.push(';');
        }

        filter_graph.push_str(&format!(
            "[{input}]format=rgba[ov{index}];[{previous}][ov{index}]overlay=0:0:enable='between(t,{start:.2},{end:.2})'[{next}]"
        ));

        previous = next;
    }

    command
        .arg("-filter_complex")
        .arg(filter_graph)
        .arg("-map")
        .arg(format!("[{previous}]"))
        .arg("-map")
        .arg("0:a?")
        .arg("-c:v")
        .arg("libx264")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-c:a")
        .arg("copy")
        .arg(output);

    let status = command
        .status()
        .with_context(|| format!("failed to start overlay burn-in for {}", video.display()))?;

    if !status.success() {
        bail!("ffmpeg failed while compositing subtitle overlays");
    }

    Ok(())
}

fn point_size_for_height(height: u32) -> u32 {
    let candidate = height / 17;
    candidate.max(28)
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

    reasons
}

fn line_style_for_index(style: &BurninStyle, index: usize, line: &str) -> LineStyle {
    let role = style
        .line_order
        .get(index)
        .map(String::as_str)
        .unwrap_or("");
    let mut resolved = style.line_styles.get(role).cloned().unwrap_or_default();
    if resolved.font.is_none() {
        resolved.font = Some(select_overlay_font(line).to_string());
    }
    resolved
}

fn select_overlay_font(text: &str) -> &'static str {
    let candidates = if text.chars().any(is_cjk_or_korean_or_japanese) {
        &CJK_FONT_CANDIDATES[..]
    } else {
        &LATIN_FONT_CANDIDATES[..]
    };

    candidates
        .iter()
        .copied()
        .find(|path| Path::new(path).exists())
        .unwrap_or(LATIN_FONT_CANDIDATES[0])
}

fn resolve_overlay_font(requested: &str, sample_text: &str) -> String {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return select_overlay_font(sample_text).to_string();
    }

    if Path::new(trimmed).exists() {
        return trimmed.to_string();
    }

    if let Some(path) = named_font_candidates(trimmed)
        .iter()
        .copied()
        .find(|path| Path::new(path).exists())
    {
        return path.to_string();
    }

    select_overlay_font(sample_text).to_string()
}

fn named_font_candidates(requested: &str) -> &'static [&'static str] {
    match normalize_font_name(requested).as_str() {
        "pingfangsc" | "pingfang" => &PINGFANG_FONT_CANDIDATES,
        "arial" => &ARIAL_FONT_CANDIDATES,
        "hiraginosans" | "hiraginosansgb" => &HIRAGINO_SANS_FONT_CANDIDATES,
        _ => &[],
    }
}

fn normalize_font_name(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_cjk_or_korean_or_japanese(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0x31F0..=0x31FF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFF66..=0xFF9D
    )
}

#[cfg(test)]
mod tests {
    use super::{
        BurninStyle, LineStyle, is_cjk_or_korean_or_japanese, line_style_for_index,
        multiline_line_padding, named_font_candidates, overlay_renderer_reasons,
        select_overlay_font,
    };
    use std::collections::HashMap;

    #[test]
    fn detects_cjk_script() {
        assert!(is_cjk_or_korean_or_japanese('不'));
        assert!(is_cjk_or_korean_or_japanese('あ'));
        assert!(is_cjk_or_korean_or_japanese('한'));
        assert!(!is_cjk_or_korean_or_japanese('A'));
    }

    #[test]
    fn prefers_cjk_font_for_cjk_text() {
        let font = select_overlay_font("不好了麻醉剂用完了");
        assert!(!font.contains("Helvetica"));
    }

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
            line_order: vec!["source".to_string(), "en".to_string()],
            line_styles,
        };

        let line_style = line_style_for_index(&style, 1, "English");
        assert_eq!(line_style.font.as_deref(), Some("Arial"));
        assert_eq!(line_style.size, Some(30));
    }

    #[test]
    fn maps_named_font_aliases_to_candidates() {
        assert!(!named_font_candidates("PingFang SC").is_empty());
        assert!(!named_font_candidates("Arial").is_empty());
    }

    #[test]
    fn multiline_padding_stays_tight() {
        assert_eq!(multiline_line_padding(30), 1);
        assert_eq!(multiline_line_padding(42), 1);
        assert_eq!(multiline_line_padding(60), 2);
    }

    #[test]
    fn line_spacing_forces_overlay_renderer() {
        let style = BurninStyle {
            font: None,
            colour: None,
            size: None,
            line_spacing: Some(3),
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
            line_order: vec!["source".to_string()],
            line_styles: HashMap::from([("source".to_string(), LineStyle::default())]),
        };

        let reasons = overlay_renderer_reasons(&style);
        assert_eq!(reasons, vec!["per_line_styles", "line_spacing", "colour"]);
    }
}
