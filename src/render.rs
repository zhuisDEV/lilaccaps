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

pub fn burn_in_subtitles(
    runtime_home: &Path,
    video: &Path,
    subs: &Path,
    output: &Path,
) -> Result<()> {
    ensure_ffmpeg_available()?;

    if ffmpeg_supports_filter("subtitles")? {
        return burn_in_with_subtitles_filter(video, subs, output);
    }

    burn_in_with_overlay_fallback(runtime_home, video, subs, output)
}

fn burn_in_with_subtitles_filter(video: &Path, subs: &Path, output: &Path) -> Result<()> {
    let filter = subtitles_filter(subs);
    let status = Command::new("ffmpeg")
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
) -> Result<()> {
    ensure_dependency(MAGICK_DEPENDENCY)?;

    let cues = parse_srt_file(subs)?;
    if cues.is_empty() {
        bail!("subtitle file contained no cues: {}", subs.display());
    }

    let (width, height) = video_size(video)?;
    let work_dir = tmp_dir(runtime_home).join("burnin-overlays");
    ensure_dir(&work_dir)?;

    let overlays = render_overlay_images(&work_dir, width, height, &cues)?;
    burn_in_with_overlay_images(video, &overlays, output)
}

fn render_overlay_images(
    work_dir: &Path,
    width: u32,
    height: u32,
    cues: &[SubtitleCue],
) -> Result<Vec<(SubtitleCue, PathBuf)>> {
    let mut overlays = Vec::with_capacity(cues.len());

    for cue in cues {
        let font_path = select_overlay_font(&cue.text);
        let image_path = work_dir.join(format!("cue-{:04}.png", cue.index));
        let image_target = format!("PNG32:{}", image_path.display());
        let label = format!("label:{}", cue.text);
        let status = Command::new("magick")
            .arg("-background")
            .arg("none")
            .arg("-font")
            .arg(font_path)
            .arg("-fill")
            .arg("white")
            .arg("-stroke")
            .arg("black")
            .arg("-strokewidth")
            .arg("2")
            .arg("-pointsize")
            .arg(point_size_for_height(height).to_string())
            .arg(&label)
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
            .arg(&image_target)
            .status()
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

fn burn_in_with_overlay_images(
    video: &Path,
    overlays: &[(SubtitleCue, PathBuf)],
    output: &Path,
) -> Result<()> {
    if overlays.is_empty() {
        bail!("no subtitle overlays were generated");
    }

    let mut command = Command::new("ffmpeg");
    command.arg("-y").arg("-i").arg(video);

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
    use super::{is_cjk_or_korean_or_japanese, select_overlay_font};

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
}
