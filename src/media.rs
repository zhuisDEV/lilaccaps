use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::runtime::{FFMPEG_DEPENDENCY, FFPROBE_DEPENDENCY, ensure_dependency};

pub fn ensure_ffmpeg_available() -> Result<()> {
    ensure_dependency(FFMPEG_DEPENDENCY)?;
    ensure_dependency(FFPROBE_DEPENDENCY)?;
    Ok(())
}

pub fn extract_audio_to_wav(input: &Path, output: &Path) -> Result<()> {
    ensure_ffmpeg_available()?;

    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg("-vn")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg(output)
        .status()
        .with_context(|| {
            format!(
                "failed to start ffmpeg audio extraction for {}",
                input.display()
            )
        })?;

    if !status.success() {
        bail!(
            "ffmpeg failed while extracting mono wav audio from {}",
            input.display()
        );
    }

    Ok(())
}

pub fn subtitles_filter(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let escaped = normalized
        .replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace('\'', "\\'")
        .replace(',', "\\,")
        .replace('[', "\\[")
        .replace(']', "\\]");
    format!("subtitles=filename='{escaped}'")
}

pub fn ffmpeg_supports_filter(name: &str) -> Result<bool> {
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-filters")
        .output()
        .context("failed to inspect ffmpeg filters")?;

    if !output.status.success() {
        bail!("ffmpeg -filters failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .any(|line| line.contains(&format!(" {name} "))))
}

pub fn video_size(path: &Path) -> Result<(u32, u32)> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height")
        .arg("-of")
        .arg("csv=p=0:s=x")
        .arg(path)
        .output()
        .with_context(|| format!("failed to probe video size for {}", path.display()))?;

    if !output.status.success() {
        bail!("ffprobe failed while probing {}", path.display());
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let (width, height) = raw
        .split_once('x')
        .ok_or_else(|| anyhow::anyhow!("unexpected ffprobe video size output: {raw}"))?;

    Ok((width.parse()?, height.parse()?))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::subtitles_filter;

    #[test]
    fn escapes_subtitles_filter_path() {
        let filter = subtitles_filter(Path::new("/tmp/a:b'[c].srt"));
        assert!(filter.contains("\\:"));
        assert!(filter.contains("\\'"));
        assert!(filter.contains("\\["));
        assert!(filter.contains("\\]"));
    }
}
