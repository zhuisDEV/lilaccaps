use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::watermark::{
    WatermarkPosition, WatermarkSource, WatermarkStyle, apply_watermark, normalized_opacity,
};

#[derive(Debug, Clone)]
pub struct WatermarkOutput {
    pub video: PathBuf,
    pub output: PathBuf,
    pub watermark: String,
    pub position: &'static str,
    pub opacity: f32,
    pub size: u32,
    pub margin: u32,
    pub status: &'static str,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    video: PathBuf,
    output: Option<PathBuf>,
    text: Option<String>,
    image: Option<PathBuf>,
    position: WatermarkPosition,
    opacity: f32,
    size: u32,
    margin: u32,
    colour: String,
    font: Option<String>,
) -> Result<WatermarkOutput> {
    if !video.exists() {
        bail!("video input does not exist: {}", video.display());
    }

    let source = resolve_source(text, image)?;
    if let WatermarkSource::Image(path) = &source
        && !path.exists()
    {
        bail!("watermark image does not exist: {}", path.display());
    }

    let opacity = normalized_opacity(opacity)?;
    let output = output.unwrap_or_else(|| default_output_path(&video));
    if output_matches_input(&video, &output)? {
        bail!(
            "watermark output must be different from input video: {}",
            video.display()
        );
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create output directory for watermark {}",
                output.display()
            )
        })?;
    }

    let style = WatermarkStyle {
        position,
        opacity,
        size,
        margin,
        colour,
        font,
    };
    apply_watermark(&video, &output, &source, &style)?;

    Ok(WatermarkOutput {
        video,
        output,
        watermark: source.label(),
        position: position.label(),
        opacity,
        size,
        margin,
        status: "rendered",
    })
}

fn resolve_source(text: Option<String>, image: Option<PathBuf>) -> Result<WatermarkSource> {
    match (text, image) {
        (Some(text), None) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                bail!("text watermark cannot be empty");
            }
            Ok(WatermarkSource::Text(text))
        }
        (None, Some(image)) => Ok(WatermarkSource::Image(image)),
        (None, None) => bail!("watermark requires exactly one of --text or --image"),
        (Some(_), Some(_)) => bail!("watermark accepts only one of --text or --image"),
    }
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
    video.with_file_name(format!("{stem}.watermarked.{extension}"))
}

fn output_matches_input(video: &Path, output: &Path) -> Result<bool> {
    if video == output {
        return Ok(true);
    }

    if !output.exists() {
        return Ok(false);
    }

    let video = fs::canonicalize(video)
        .with_context(|| format!("failed to canonicalize video input {}", video.display()))?;
    let output = fs::canonicalize(output).with_context(|| {
        format!(
            "failed to canonicalize watermark output {}",
            output.display()
        )
    })?;
    Ok(video == output)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{default_output_path, output_matches_input, resolve_source};
    use crate::watermark::WatermarkSource;

    fn temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "lilaccaps-watermark-test-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn default_output_path_adds_watermarked_suffix() {
        let path = default_output_path(Path::new("/tmp/input.mp4"));
        assert_eq!(path, Path::new("/tmp/input.watermarked.mp4"));
    }

    #[test]
    fn text_source_trims_input() {
        let source = resolve_source(Some("  lilac  ".to_string()), None).expect("valid text");
        assert!(matches!(source, WatermarkSource::Text(text) if text == "lilac"));
    }

    #[test]
    fn source_requires_exactly_one_input() {
        let err = resolve_source(None, None).unwrap_err();
        assert!(err.to_string().contains("exactly one"));

        let err =
            resolve_source(Some("lilac".to_string()), Some("/tmp/logo.png".into())).unwrap_err();
        assert!(err.to_string().contains("only one"));
    }

    #[test]
    fn output_match_check_catches_same_file_by_canonical_path() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        let input = dir.join("input.mp4");
        std::fs::write(&input, b"demo").expect("temp input should be written");
        let canonical_input = std::fs::canonicalize(&input).expect("input should canonicalize");
        let output = dir.join("output.mp4");

        assert!(output_matches_input(&input, &input).expect("same path should compare"));
        assert!(
            output_matches_input(&input, &canonical_input).expect("canonical path should compare")
        );
        assert!(!output_matches_input(&input, &output).expect("missing output should not match"));

        std::fs::remove_dir_all(&dir).expect("temp dir should be removed");
    }
}
