use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::runtime::{ScopedTempPath, UV_DEPENDENCY, ensure_dependency, models_dir, tmp_dir};
use crate::subtitles::{SubtitleCue, TimedWord};

const HELPER_SOURCE: &str = include_str!("../python/faster_whisper_helper.py");

#[derive(Debug, Deserialize)]
struct HelperWord {
    text: String,
    start: f64,
    end: f64,
}

#[derive(Debug, Deserialize)]
struct HelperSegment {
    text: String,
    start: f64,
    end: f64,
}

#[derive(Debug, Deserialize)]
struct HelperOutput {
    language: String,
    duration: f64,
    duration_after_vad: f64,
    segments: Vec<HelperSegment>,
    words: Vec<HelperWord>,
}

#[derive(Debug)]
pub struct FasterWhisperOutput {
    pub language: String,
    pub duration_cs: i64,
    pub duration_after_vad_cs: i64,
    pub segments: Vec<SubtitleCue>,
    pub words: Vec<TimedWord>,
}

pub fn transcribe(
    runtime_home: &Path,
    audio_path: &Path,
    model: &str,
    language: Option<&str>,
) -> Result<FasterWhisperOutput> {
    ensure_dependency(UV_DEPENDENCY)?;
    let helper = write_helper(runtime_home)?;
    let model_root = models_dir(runtime_home).join("faster-whisper");
    fs::create_dir_all(&model_root).with_context(|| {
        format!(
            "failed to create faster-whisper model directory {}",
            model_root.display()
        )
    })?;

    let mut command = Command::new("uv");
    command
        .arg("run")
        .arg("--script")
        .arg(helper.path())
        .arg("--audio")
        .arg(audio_path)
        .arg("--model")
        .arg(model)
        .arg("--download-root")
        .arg(&model_root);
    if let Some(language) = language {
        command.arg("--language").arg(language);
    }
    let output = command
        .output()
        .context("failed to start uv-managed faster-whisper helper")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "faster-whisper helper failed with {}: {}",
            output.status,
            stderr.trim()
        );
    }
    let decoded: HelperOutput = serde_json::from_slice(&output.stdout)
        .context("failed to decode faster-whisper helper JSON")?;
    let words = decoded
        .words
        .into_iter()
        .filter_map(|word| timed_word(word.text, word.start, word.end))
        .collect::<Vec<_>>();
    let segments = decoded
        .segments
        .into_iter()
        .enumerate()
        .filter_map(|(index, segment)| {
            subtitle_cue(index + 1, segment.text, segment.start, segment.end)
        })
        .collect::<Vec<_>>();

    Ok(FasterWhisperOutput {
        language: decoded.language,
        duration_cs: seconds_to_centiseconds(decoded.duration),
        duration_after_vad_cs: seconds_to_centiseconds(decoded.duration_after_vad),
        segments,
        words,
    })
}

pub fn check(runtime_home: &Path) -> Result<()> {
    ensure_dependency(UV_DEPENDENCY)?;
    let helper = write_helper(runtime_home)?;
    let model_root = models_dir(runtime_home).join("faster-whisper");
    fs::create_dir_all(&model_root).with_context(|| {
        format!(
            "failed to create faster-whisper model directory {}",
            model_root.display()
        )
    })?;
    let output = Command::new("uv")
        .arg("run")
        .arg("--script")
        .arg(helper.path())
        .arg("--audio")
        .arg(helper.path())
        .arg("--model")
        .arg("large-v3-turbo")
        .arg("--download-root")
        .arg(model_root)
        .arg("--check")
        .output()
        .context("failed to start faster-whisper dependency check")?;
    if !output.status.success() {
        bail!(
            "faster-whisper dependency check failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn write_helper(runtime_home: &Path) -> Result<ScopedTempPath> {
    let temp_root = tmp_dir(runtime_home);
    fs::create_dir_all(&temp_root).with_context(|| {
        format!(
            "failed to create runtime temp directory {}",
            temp_root.display()
        )
    })?;
    let helper = ScopedTempPath::file(&temp_root, "faster-whisper", Some("py"));
    fs::write(helper.path(), HELPER_SOURCE)
        .with_context(|| format!("failed to write helper {}", helper.path().display()))?;
    Ok(helper)
}

fn seconds_to_centiseconds(seconds: f64) -> i64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    (seconds * 100.0).round().min(i64::MAX as f64) as i64
}

fn timed_word(text: String, start: f64, end: f64) -> Option<TimedWord> {
    let start_cs = seconds_to_centiseconds(start);
    let end_cs = seconds_to_centiseconds(end);
    (end_cs > start_cs && !text.is_empty()).then_some(TimedWord {
        start_cs,
        end_cs,
        text,
    })
}

fn subtitle_cue(index: usize, text: String, start: f64, end: f64) -> Option<SubtitleCue> {
    let start_cs = seconds_to_centiseconds(start);
    let end_cs = seconds_to_centiseconds(end);
    let text = text.trim().to_string();
    (end_cs > start_cs && !text.is_empty()).then_some(SubtitleCue {
        index,
        start_cs,
        end_cs,
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::{seconds_to_centiseconds, subtitle_cue, timed_word};

    #[test]
    fn converts_faster_whisper_seconds_to_centiseconds() {
        assert_eq!(seconds_to_centiseconds(1.235), 124);
        assert_eq!(seconds_to_centiseconds(f64::NAN), 0);
    }

    #[test]
    fn rejects_empty_or_invalid_helper_items() {
        assert!(timed_word(String::new(), 0.0, 1.0).is_none());
        assert!(subtitle_cue(1, "hello".to_string(), 2.0, 1.0).is_none());
    }
}
