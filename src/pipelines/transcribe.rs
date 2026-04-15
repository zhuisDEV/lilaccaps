use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;

use anyhow::{Context, Result, bail};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, get_lang_id,
    get_lang_str, install_logging_hooks,
};

use crate::config::load_or_init_config;
use crate::media::{ensure_ffmpeg_available, extract_audio_to_wav};
use crate::model::ensure_model_downloaded;
use crate::runtime::{ensure_dir, tmp_dir};
use crate::subtitles::{SubtitleCue, write_srt_file};

static WHISPER_LOGGING_HOOKS: Once = Once::new();

#[derive(Debug, Clone)]
pub struct TranscribeOutput {
    pub input: PathBuf,
    pub output: PathBuf,
    pub model_path: PathBuf,
    pub language: String,
    pub fallback_language_used: bool,
    pub fallback_decoding_used: bool,
    pub decoding_strategy: &'static str,
    pub status: &'static str,
}

#[derive(Debug, Clone)]
struct AttemptDiagnostics {
    language: String,
    decoding_strategy: &'static str,
    segment_count: usize,
    non_empty_segments: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttemptPlan {
    language: String,
    fallback_language_used: bool,
    fallback_decoding_used: bool,
    strategy: DecodeStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeStrategy {
    BeamSearch,
    Greedy,
}

impl DecodeStrategy {
    fn label(self) -> &'static str {
        match self {
            Self::BeamSearch => "beam",
            Self::Greedy => "greedy",
        }
    }
}

impl AttemptDiagnostics {
    fn summary(&self) -> String {
        format!(
            "language={} decoding={} total_segments={} non_empty_segments={} blank_segments={}",
            self.language,
            self.decoding_strategy,
            self.segment_count,
            self.non_empty_segments,
            self.segment_count.saturating_sub(self.non_empty_segments)
        )
    }
}

impl AttemptPlan {
    fn new(
        language: impl Into<String>,
        fallback_language_used: bool,
        fallback_decoding_used: bool,
        strategy: DecodeStrategy,
    ) -> Self {
        Self {
            language: language.into(),
            fallback_language_used,
            fallback_decoding_used,
            strategy,
        }
    }
}

pub fn run(
    input: PathBuf,
    config_path: Option<PathBuf>,
    output: Option<PathBuf>,
    lang: Option<String>,
) -> Result<TranscribeOutput> {
    if !input.exists() {
        bail!("input media does not exist: {}", input.display());
    }

    ensure_ffmpeg_available()?;
    WHISPER_LOGGING_HOOKS.call_once(install_logging_hooks);
    let loaded = load_or_init_config(config_path)?;
    ensure_dir(&loaded.paths.runtime_home)?;
    ensure_dir(&tmp_dir(&loaded.paths.runtime_home))?;

    let output = output.unwrap_or_else(|| default_output_path(&input));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create output directory for transcription {}",
                output.display()
            )
        })?;
    }

    let model_path = ensure_model_downloaded(&loaded.paths, &loaded.config)?;
    let audio_path = temp_audio_path(&loaded.paths.runtime_home, &input);
    let language = resolve_language(lang.as_deref(), Some(&loaded.config.transcribe.language))?;
    extract_audio_to_wav(&input, &audio_path)?;
    let (
        cues,
        effective_language,
        fallback_language_used,
        fallback_decoding_used,
        decoding_strategy,
    ) = transcribe_to_cues(&model_path, &audio_path, language.as_deref())?;
    write_srt_file(&output, &cues)?;

    Ok(TranscribeOutput {
        input,
        output,
        model_path,
        language: effective_language,
        fallback_language_used,
        fallback_decoding_used,
        decoding_strategy,
        status: "generated",
    })
}

fn default_output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("transcript");
    input.with_file_name(format!("{stem}.srt"))
}

fn temp_audio_path(runtime_home: &Path, input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("input");
    tmp_dir(runtime_home).join(format!("{stem}.mono16k.wav"))
}

fn transcribe_to_cues(
    model_path: &Path,
    audio_path: &Path,
    language: Option<&str>,
) -> Result<(Vec<SubtitleCue>, String, bool, bool, &'static str)> {
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
    let detected_language = match language {
        Some(_) => detect_language(&ctx, &audio).ok(),
        None => Some(detect_language(&ctx, &audio)?),
    };
    let attempts = build_attempts(language, detected_language.as_deref());

    let mut diagnostics = Vec::new();
    for attempt in attempts {
        let (cues, attempt_diagnostics) = transcribe_once(
            &ctx,
            &audio,
            Some(attempt.language.as_str()),
            attempt.strategy,
        )?;
        if !cues.is_empty() {
            return Ok((
                cues,
                attempt.language,
                attempt.fallback_language_used,
                attempt.fallback_decoding_used,
                attempt.strategy.label(),
            ));
        }
        diagnostics.push(attempt_diagnostics);
    }

    let summary = diagnostics
        .iter()
        .map(AttemptDiagnostics::summary)
        .collect::<Vec<_>>()
        .join(" | ");
    bail!(
        "whisper returned no subtitle segments after trying beam and greedy decoding. attempt_diagnostics = {summary}"
    )
}

fn build_attempts(
    requested_language: Option<&str>,
    detected_language: Option<&str>,
) -> Vec<AttemptPlan> {
    let primary_language = requested_language
        .or(detected_language)
        .expect("transcription attempts require either a requested or detected language");
    let mut attempts = vec![
        AttemptPlan::new(primary_language, false, false, DecodeStrategy::BeamSearch),
        AttemptPlan::new(primary_language, false, true, DecodeStrategy::Greedy),
    ];

    if let (Some(requested_language), Some(detected_language)) =
        (requested_language, detected_language)
    {
        if detected_language != requested_language {
            attempts.push(AttemptPlan::new(
                detected_language,
                true,
                false,
                DecodeStrategy::BeamSearch,
            ));
            attempts.push(AttemptPlan::new(
                detected_language,
                true,
                true,
                DecodeStrategy::Greedy,
            ));
        }
    }

    attempts
}

fn detect_language(ctx: &WhisperContext, audio: &[f32]) -> Result<String> {
    if !ctx.is_multilingual() {
        return Ok("en".to_string());
    }

    let threads = num_cpus::get_physical().max(1);
    let mut state = ctx
        .create_state()
        .context("failed to create whisper state for language detection")?;
    state
        .pcm_to_mel(audio, threads)
        .context("failed to prepare audio for whisper language detection")?;
    let (lang_id, _) = state
        .lang_detect(0, threads)
        .context("failed to auto-detect whisper language")?;

    get_lang_str(lang_id)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("whisper returned unknown language id {lang_id}"))
}

fn transcribe_once(
    ctx: &WhisperContext,
    audio: &[f32],
    language: Option<&str>,
    strategy: DecodeStrategy,
) -> Result<(Vec<SubtitleCue>, AttemptDiagnostics)> {
    let mut state = ctx
        .create_state()
        .context("failed to create whisper state")?;

    let mut params = match strategy {
        DecodeStrategy::BeamSearch => FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: -1.0,
        }),
        DecodeStrategy::Greedy => FullParams::new(SamplingStrategy::Greedy { best_of: 3 }),
    };
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_translate(false);
    params.set_language(language);
    // `detect_language=true` makes whisper.cpp return after detection without transcribing.
    // For actual transcription we always pass an explicit language, including auto-detected ones.
    params.set_detect_language(false);
    params.set_n_threads(num_cpus::get_physical() as i32);

    state
        .full(params, audio)
        .context("failed to transcribe audio with whisper")?;

    collect_cues(&state, language, strategy)
}

fn collect_cues(
    state: &whisper_rs::WhisperState,
    language: Option<&str>,
    strategy: DecodeStrategy,
) -> Result<(Vec<SubtitleCue>, AttemptDiagnostics)> {
    let mut cues = Vec::new();
    let mut segment_count = 0usize;
    for (index, segment) in state.as_iter().enumerate() {
        segment_count += 1;
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

    let non_empty_segments = cues.len();
    Ok((
        cues,
        AttemptDiagnostics {
            language: language.unwrap_or("auto").to_string(),
            decoding_strategy: strategy.label(),
            segment_count,
            non_empty_segments,
        },
    ))
}

fn resolve_language(cli_lang: Option<&str>, config_lang: Option<&str>) -> Result<Option<String>> {
    let raw = cli_lang
        .or(config_lang)
        .unwrap_or("auto")
        .trim()
        .to_lowercase();
    if raw.is_empty() || raw == "auto" {
        return Ok(None);
    }

    if get_lang_id(&raw).is_none() {
        bail!(
            "unsupported transcription language `{raw}`; use `auto` or a Whisper language code such as `en`, `zh`, or `ja`"
        );
    }

    Ok(Some(raw))
}

#[cfg(test)]
mod tests {
    use super::{
        AttemptDiagnostics, AttemptPlan, DecodeStrategy, build_attempts, resolve_language,
    };

    #[test]
    fn cli_language_overrides_config_language() {
        let resolved = resolve_language(Some("zh"), Some("en")).expect("language should resolve");
        assert_eq!(resolved.as_deref(), Some("zh"));
    }

    #[test]
    fn auto_language_maps_to_detection() {
        let resolved = resolve_language(Some("auto"), Some("en")).expect("language should resolve");
        assert_eq!(resolved, None);
    }

    #[test]
    fn invalid_language_is_rejected() {
        let err = resolve_language(Some("notalanguage"), None).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported transcription language")
        );
    }

    #[test]
    fn decode_strategy_labels_are_stable() {
        assert_eq!(DecodeStrategy::BeamSearch.label(), "beam");
        assert_eq!(DecodeStrategy::Greedy.label(), "greedy");
    }

    #[test]
    fn attempt_diagnostics_summary_includes_blank_segments() {
        let diagnostics = AttemptDiagnostics {
            language: "zh".to_string(),
            decoding_strategy: "beam",
            segment_count: 4,
            non_empty_segments: 1,
        };
        let summary = diagnostics.summary();
        assert!(summary.contains("language=zh"));
        assert!(summary.contains("decoding=beam"));
        assert!(summary.contains("blank_segments=3"));
    }

    #[test]
    fn attempt_plan_marks_decoding_fallback_only_for_greedy() {
        let beam = AttemptPlan::new("zh", false, false, DecodeStrategy::BeamSearch);
        let greedy = AttemptPlan::new("zh", false, true, DecodeStrategy::Greedy);

        assert!(!beam.fallback_decoding_used);
        assert!(greedy.fallback_decoding_used);
    }

    #[test]
    fn auto_language_attempts_use_detected_language_only() {
        let attempts = build_attempts(None, Some("zh"));

        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0],
            AttemptPlan::new("zh", false, false, DecodeStrategy::BeamSearch)
        );
        assert_eq!(
            attempts[1],
            AttemptPlan::new("zh", false, true, DecodeStrategy::Greedy)
        );
    }

    #[test]
    fn explicit_language_attempts_include_detected_fallback_when_different() {
        let attempts = build_attempts(Some("ja"), Some("zh"));

        assert_eq!(attempts.len(), 4);
        assert_eq!(
            attempts[0],
            AttemptPlan::new("ja", false, false, DecodeStrategy::BeamSearch)
        );
        assert_eq!(
            attempts[1],
            AttemptPlan::new("ja", false, true, DecodeStrategy::Greedy)
        );
        assert_eq!(
            attempts[2],
            AttemptPlan::new("zh", true, false, DecodeStrategy::BeamSearch)
        );
        assert_eq!(
            attempts[3],
            AttemptPlan::new("zh", true, true, DecodeStrategy::Greedy)
        );
    }
}
