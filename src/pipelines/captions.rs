use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::config::load_or_init_config;
use crate::media::{ensure_ffmpeg_available, extract_audio_to_wav};
use crate::model::ensure_model_downloaded;
use crate::runtime::{ensure_dir, tmp_dir};
use crate::subtitles::{SubtitleCue, write_srt_file};

#[derive(Debug, Clone)]
pub struct CaptionsOutput {
    pub input: PathBuf,
    pub output: PathBuf,
    pub model_path: PathBuf,
    pub status: &'static str,
}

pub fn run(
    input: PathBuf,
    config_path: Option<PathBuf>,
    output: Option<PathBuf>,
) -> Result<CaptionsOutput> {
    if !input.exists() {
        bail!("input media does not exist: {}", input.display());
    }

    ensure_ffmpeg_available()?;
    let loaded = load_or_init_config(config_path)?;
    ensure_dir(&loaded.paths.runtime_home)?;
    ensure_dir(&tmp_dir(&loaded.paths.runtime_home))?;

    let output = output.unwrap_or_else(|| default_output_path(&input));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create output directory for captions {}",
                output.display()
            )
        })?;
    }

    let model_path = ensure_model_downloaded(&loaded.paths, &loaded.config)?;
    let audio_path = temp_audio_path(&loaded.paths.runtime_home, &input);
    extract_audio_to_wav(&input, &audio_path)?;
    let cues = transcribe_to_cues(&model_path, &audio_path)?;
    write_srt_file(&output, &cues)?;

    Ok(CaptionsOutput {
        input,
        output,
        model_path,
        status: "generated",
    })
}

fn default_output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("captions");
    input.with_file_name(format!("{stem}.srt"))
}

fn temp_audio_path(runtime_home: &Path, input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("input");
    tmp_dir(runtime_home).join(format!("{stem}.mono16k.wav"))
}

fn transcribe_to_cues(model_path: &Path, audio_path: &Path) -> Result<Vec<SubtitleCue>> {
    let samples: Vec<i16> = hound::WavReader::open(audio_path)
        .with_context(|| format!("failed to open extracted wav {}", audio_path.display()))?
        .into_samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read wav samples from {}", audio_path.display()))?;

    let mut audio = vec![0.0f32; samples.len()];
    whisper_rs::convert_integer_to_float_audio(&samples, &mut audio)
        .context("failed to convert wav samples to whisper input format")?;

    let ctx = WhisperContext::new_with_params(
        model_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("model path is not valid UTF-8"))?,
        WhisperContextParameters::default(),
    )
    .with_context(|| format!("failed to load whisper model {}", model_path.display()))?;
    let mut state = ctx
        .create_state()
        .context("failed to create whisper state")?;

    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: -1.0,
    });
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_translate(false);
    params.set_n_threads(num_cpus::get_physical() as i32);

    state
        .full(params, &audio)
        .context("failed to transcribe audio with whisper")?;

    let mut cues = Vec::new();
    for (index, segment) in state.as_iter().enumerate() {
        let text = segment
            .to_str_lossy()
            .context("failed to decode whisper segment text")?
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }

        cues.push(SubtitleCue {
            index: index + 1,
            start_cs: segment.start_timestamp(),
            end_cs: segment.end_timestamp(),
            text,
        });
    }

    if cues.is_empty() {
        bail!("whisper returned no subtitle segments");
    }

    Ok(cues)
}
