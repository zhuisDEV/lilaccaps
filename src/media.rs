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

    let output_result = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
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
        .output()
        .with_context(|| {
            format!(
                "failed to start ffmpeg audio extraction for {}",
                input.display()
            )
        })?;

    if !output_result.status.success() {
        let stderr = String::from_utf8_lossy(&output_result.stderr)
            .trim()
            .to_string();
        if stderr.is_empty() {
            bail!(
                "ffmpeg failed while extracting mono wav audio from {}",
                input.display()
            );
        }
        bail!(
            "ffmpeg failed while extracting mono wav audio from {}: {}",
            input.display(),
            stderr
        );
    }

    Ok(())
}

pub fn subtitles_filter(
    path: &Path,
    font: Option<&str>,
    size: Option<u32>,
    outline_colour: Option<&str>,
    outline_width: Option<u32>,
) -> String {
    let mut filter = format!(
        "subtitles=filename={}",
        escape_filter_option(&path.to_string_lossy())
    );
    let mut style = Vec::new();

    if let Some(font) = font {
        style.push(format!("FontName={font}"));
    }

    if let Some(size) = size {
        style.push(format!("FontSize={size}"));
    }

    if let Some(width) = outline_width {
        style.push("BorderStyle=1".to_string());
        style.push(format!("Outline={width}"));

        if let Some(colour) = outline_colour.and_then(ass_colour) {
            style.push(format!("OutlineColour={colour}"));
        }
    }

    if !style.is_empty() {
        filter.push_str(":force_style=");
        filter.push_str(&escape_filter_option(&style.join(",")));
    }

    filter
}

pub fn ass_colour(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(hex) = trimmed.strip_prefix('#') {
        return ass_colour_from_hex(hex);
    }

    if let Some(rgb) = ass_colour_from_rgb_function(trimmed) {
        return Some(rgb);
    }

    let (red, green, blue) = match trimmed.to_ascii_lowercase().as_str() {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "yellow" => (255, 255, 0),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "lime" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        _ => return None,
    };

    Some(format_ass_colour(red, green, blue, 0))
}

fn ass_colour_from_hex(hex: &str) -> Option<String> {
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let (red, green, blue, alpha) = match hex.len() {
        6 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
            0,
        ),
        8 => {
            let css_alpha = u8::from_str_radix(&hex[6..8], 16).ok()?;
            (
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
                255u8.saturating_sub(css_alpha),
            )
        }
        _ => return None,
    };

    Some(format_ass_colour(red, green, blue, alpha))
}

fn ass_colour_from_rgb_function(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let body = lower
        .strip_prefix("rgb(")
        .and_then(|value| value.strip_suffix(')'))?;
    let channels = body
        .split(',')
        .map(str::trim)
        .map(str::parse::<u8>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;

    if channels.len() != 3 {
        return None;
    }

    Some(format_ass_colour(channels[0], channels[1], channels[2], 0))
}

fn format_ass_colour(red: u8, green: u8, blue: u8, alpha: u8) -> String {
    format!("&H{alpha:02X}{blue:02X}{green:02X}{red:02X}")
}

pub fn escape_filter_option(value: &str) -> String {
    fn escape_token(value: &str, separators: &str) -> String {
        let mut escaped = String::new();
        for character in value.chars() {
            if matches!(character, '\\' | '\'')
                || character.is_whitespace()
                || separators.contains(character)
            {
                escaped.push('\\');
            }
            escaped.push(character);
        }
        escaped
    }

    // Escape option values before embedding them in the enclosing filtergraph.
    // Command passes argv directly, so no shell quoting layer is needed.
    // https://ffmpeg.org/ffmpeg-filters.html#Notes-on-filtergraph-escaping
    escape_token(&escape_token(value, ":"), "[],;")
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
        let filter = subtitles_filter(Path::new("/tmp/a:b'[c].srt"), None, None, None, None);
        assert!(filter.contains("\\:"));
        assert!(filter.contains("\\'"));
        assert!(filter.contains("\\["));
        assert!(filter.contains("\\]"));
    }

    #[test]
    fn includes_force_style_when_font_or_size_is_set() {
        let filter = subtitles_filter(
            Path::new("/tmp/input.srt"),
            Some("PingFang SC"),
            Some(42),
            None,
            None,
        );
        assert!(filter.contains("force_style"));
        assert!(filter.contains("FontName=PingFang\\\\\\ SC"));
        assert!(filter.contains("FontSize=42"));
    }

    #[test]
    fn includes_outline_force_style_when_outline_is_set() {
        let filter = subtitles_filter(
            Path::new("/tmp/input.srt"),
            None,
            None,
            Some("black"),
            Some(2),
        );
        assert!(filter.contains("BorderStyle=1"));
        assert!(filter.contains("Outline=2"));
        assert!(filter.contains("OutlineColour=&H00000000"));
    }

    #[test]
    fn disabled_outline_explicitly_overrides_the_renderer_default() {
        let filter = subtitles_filter(Path::new("input.srt"), None, None, None, Some(0));
        assert!(filter.contains("Outline=0"));
    }

    #[test]
    fn malformed_hex_colours_are_rejected_without_slicing_unicode() {
        for colour in ["#a€bb", "#中中", "#ffzz00", "#😀abcd"] {
            assert_eq!(super::ass_colour(colour), None);
        }
    }

    #[test]
    fn converts_hex_and_rgb_colours_to_ass_bgr() {
        assert_eq!(super::ass_colour("#ffd54f").as_deref(), Some("&H004FD5FF"));
        assert_eq!(
            super::ass_colour("rgb(255,220,120)").as_deref(),
            Some("&H0078DCFF")
        );
        assert_eq!(
            super::ass_colour("#00000080").as_deref(),
            Some("&H7F000000")
        );
    }
}
