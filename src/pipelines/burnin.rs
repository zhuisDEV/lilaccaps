use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::load_config;
use crate::render::burn_in_subtitles;

#[derive(Debug, Clone)]
pub struct BurninOutput {
    pub video: PathBuf,
    pub subs: PathBuf,
    pub output: PathBuf,
    pub status: &'static str,
}

pub fn run(
    video: PathBuf,
    config_path: Option<PathBuf>,
    subs: PathBuf,
    output: Option<PathBuf>,
) -> Result<BurninOutput> {
    if !video.exists() {
        bail!("video input does not exist: {}", video.display());
    }
    if !subs.exists() {
        bail!("subtitle input does not exist: {}", subs.display());
    }

    let loaded = load_config(config_path)?;

    let output = output.unwrap_or_else(|| default_output_path(&video));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create output directory for burnin {}",
                output.display()
            )
        })?;
    }

    burn_in_subtitles(&loaded.paths.runtime_home, &video, &subs, &output)?;

    Ok(BurninOutput {
        video,
        subs,
        output,
        status: "rendered",
    })
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
    video.with_file_name(format!("{stem}.burned.{extension}"))
}
