use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;

use crate::config::{Config, ConfigPaths};
use crate::runtime::{ScopedTempPath, ensure_parent_dir, models_dir, parent_dir};

const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

pub fn resolved_model_path(paths: &ConfigPaths, config: &Config) -> Result<PathBuf> {
    if let Some(path) = &config.transcribe.model.path {
        return Ok(path.clone());
    }

    let file_name = model_file_name(&config.transcribe.model.id)?;
    Ok(models_dir(&paths.runtime_home).join(file_name))
}

pub fn ensure_model_downloaded(paths: &ConfigPaths, config: &Config) -> Result<PathBuf> {
    let destination = resolved_model_path(paths, config)?;
    if destination.exists() {
        let metadata = fs::metadata(&destination)
            .with_context(|| format!("failed to inspect model file {}", destination.display()))?;
        if !metadata.is_file() {
            bail!("model path is not a file: {}", destination.display());
        }
        if metadata.len() > 0 {
            return Ok(destination);
        }
        fs::remove_file(&destination).with_context(|| {
            format!(
                "failed to remove empty model file {}",
                destination.display()
            )
        })?;
    }

    ensure_parent_dir(&destination)?;

    let url = model_url(&config.transcribe.model.id)?;
    download_to_path(&url, &destination)?;

    Ok(destination)
}

fn download_to_path(url: &str, destination: &Path) -> Result<()> {
    let client = Client::builder()
        .user_agent(format!("lilaccaps/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(3_600))
        .build()
        .context("failed to construct model download client")?;
    let mut response = client
        .get(url)
        .send()
        .with_context(|| format!("failed to request model from {url}"))?
        .error_for_status()
        .with_context(|| format!("model download failed for {url}"))?;
    let expected_size = response.content_length();
    let parent = parent_dir(destination);
    let prefix = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("whisper-model");
    let temporary = ScopedTempPath::file(parent, prefix, Some("part"));
    let file = File::create(temporary.path()).with_context(|| {
        format!(
            "failed to create temporary model file {}",
            temporary.path().display()
        )
    })?;
    let mut writer = BufWriter::new(file);
    let written = io::copy(&mut response, &mut writer)
        .context("failed while streaming model download body")?;
    writer.flush().context("failed to flush model download")?;
    writer
        .get_ref()
        .sync_all()
        .context("failed to sync model download")?;
    drop(writer);

    if written == 0 {
        bail!("model download returned an empty response from {url}");
    }
    if let Some(expected_size) = expected_size
        && written != expected_size
    {
        bail!(
            "model download was incomplete: expected {expected_size} bytes but received {written}"
        );
    }

    temporary.persist(destination)
}

fn model_url(model_id: &str) -> Result<String> {
    Ok(format!("{MODEL_BASE_URL}/{}", model_file_name(model_id)?))
}

fn model_file_name(model_id: &str) -> Result<&'static str> {
    match model_id {
        "tiny" => Ok("ggml-tiny.bin"),
        "base" => Ok("ggml-base.bin"),
        "small" => Ok("ggml-small.bin"),
        "medium" => Ok("ggml-medium.bin"),
        "tiny.en" => Ok("ggml-tiny.en.bin"),
        "base.en" => Ok("ggml-base.en.bin"),
        "small.en" => Ok("ggml-small.en.bin"),
        "medium.en" => Ok("ggml-medium.en.bin"),
        _ => bail!("unsupported whisper model id: {model_id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::model_file_name;

    #[test]
    fn resolves_model_file_name() {
        assert_eq!(
            model_file_name("base").expect("base model should resolve"),
            "ggml-base.bin"
        );
        assert_eq!(
            model_file_name("medium").expect("medium model should resolve"),
            "ggml-medium.bin"
        );
    }
}
