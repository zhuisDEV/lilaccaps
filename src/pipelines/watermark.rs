use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::runtime::{ScopedTempPath, ensure_parent_dir, parent_dir, paths_refer_to_same_file};
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
    pub renderer: &'static str,
    pub renderer_reason: String,
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
    outline_colour: String,
    outline_width: u32,
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
    if paths_refer_to_same_file(&video, &output)? {
        bail!(
            "watermark output must be different from input video: {}",
            video.display()
        );
    }
    if let WatermarkSource::Image(path) = &source
        && paths_refer_to_same_file(path, &output)?
    {
        bail!(
            "watermark output must be different from watermark image: {}",
            path.display()
        );
    }
    ensure_parent_dir(&output).with_context(|| {
        format!(
            "failed to create output directory for watermark {}",
            output.display()
        )
    })?;

    let style = WatermarkStyle {
        position,
        opacity,
        size,
        margin,
        colour,
        font,
        outline_colour,
        outline_width,
    };
    let extension = output.extension().and_then(|value| value.to_str());
    let temporary = ScopedTempPath::file(parent_dir(&output), "watermark-output", extension);
    let renderer = apply_watermark(&video, temporary.path(), &source, &style)?;
    temporary.persist(&output)?;

    Ok(WatermarkOutput {
        video,
        output,
        watermark: source.label(),
        position: position.label(),
        opacity,
        size,
        margin,
        renderer: renderer.renderer,
        renderer_reason: if renderer.reasons.is_empty() {
            "none".to_string()
        } else {
            renderer.reasons.join(",")
        },
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{default_output_path, resolve_source};
    use crate::watermark::WatermarkSource;

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
}
