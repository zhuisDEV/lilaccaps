use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::media::{ensure_ffmpeg_available, ffmpeg_supports_filter, subtitles_filter, video_size};
use crate::runtime::{command_exists, ensure_dir, tmp_dir};
use crate::subtitles::{SubtitleCue, parse_srt_file};

const DEFAULT_FONT_PATH: &str = "/System/Library/Fonts/Helvetica.ttc";

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
    if !command_exists("magick") {
        bail!("ffmpeg subtitles filter is unavailable and ImageMagick `magick` was not found");
    }

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
        let image_path = work_dir.join(format!("cue-{:04}.png", cue.index));
        let status = Command::new("magick")
            .arg("-size")
            .arg(format!("{width}x{height}"))
            .arg("xc:none")
            .arg("-font")
            .arg(DEFAULT_FONT_PATH)
            .arg("-fill")
            .arg("white")
            .arg("-stroke")
            .arg("black")
            .arg("-strokewidth")
            .arg("2")
            .arg("-pointsize")
            .arg(point_size_for_height(height).to_string())
            .arg("-gravity")
            .arg("south")
            .arg("-annotate")
            .arg("+0+40")
            .arg(&cue.text)
            .arg(&image_path)
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
            "[{previous}][{input}]overlay=0:0:enable='between(t,{start:.2},{end:.2})'[{next}]"
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
