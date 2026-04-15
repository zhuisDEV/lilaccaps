use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;

use crate::config::{Config, ConfigPaths};
use crate::runtime::{ensure_dir, models_dir};

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
        return Ok(destination);
    }

    if let Some(parent) = destination.parent() {
        ensure_dir(parent)?;
    }

    let url = model_url(&config.transcribe.model.id)?;
    download_to_path(&url, &destination)?;

    Ok(destination)
}

fn download_to_path(url: &str, destination: &Path) -> Result<()> {
    let client = Client::builder()
        .user_agent(format!("lilaccaps/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to construct model download client")?;
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("failed to request model from {url}"))?
        .error_for_status()
        .with_context(|| format!("model download failed for {url}"))?;

    let bytes = response
        .bytes()
        .context("failed to read model download body")?;
    fs::write(destination, bytes)
        .with_context(|| format!("failed to write model file {}", destination.display()))
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
