use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Once;

use anyhow::{Context, Result, bail};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, get_lang_id,
    get_lang_str, install_logging_hooks,
};

use crate::cleanup::clean_cues;
use crate::config::{
    TranscribeConfig, TranscribeEngine, TranscribeSegmentationMode, load_or_init_config,
    validate_transcribe_config,
};
use crate::faster_whisper;
use crate::media::{ensure_ffmpeg_available, extract_audio_to_wav};
use crate::model::ensure_model_downloaded;
use crate::runtime::{
    ScopedTempPath, ensure_dir, ensure_parent_dir, paths_refer_to_same_file, tmp_dir,
};
use crate::segmentation::{SpeechWindowOptions, fixed_windows, speech_windows};
use crate::subtitles::{
    CuePolicy, SubtitleCue, TimedWord, build_cues_from_timed_words, optimize_cues, validate_cues,
    write_srt_file,
};

static WHISPER_LOGGING_HOOKS: Once = Once::new();
const LANGUAGE_DETECTION_SECONDS: usize = 30;

#[derive(Debug, Clone)]
pub struct TranscribeOutput {
    pub input: PathBuf,
    pub output: PathBuf,
    pub model_path: PathBuf,
    pub engine: &'static str,
    pub model: String,
    pub language: String,
    pub fallback_language_used: bool,
    pub fallback_decoding_used: bool,
    pub decoding_strategy: &'static str,
    pub cue_count: usize,
    pub qa_warning_count: usize,
    pub cue_timing: &'static str,
    pub segmentation_strategy: &'static str,
    pub window_count: usize,
    pub cleanup: String,
    pub status: &'static str,
}

#[derive(Debug, Clone)]
struct AttemptDiagnostics {
    language: String,
    decoding_strategy: &'static str,
    segmentation_strategy: &'static str,
    window_count: usize,
    speech_region_count: usize,
    speech_threshold: Option<f32>,
    speech_coverage: f64,
    cue_timing: &'static str,
    timed_word_count: usize,
    segment_count: usize,
    non_empty_segments: usize,
}

#[derive(Debug)]
struct DecodedTranscript {
    cues: Vec<SubtitleCue>,
    language: String,
    fallback_language_used: bool,
    fallback_decoding_used: bool,
    decoding_strategy: &'static str,
    cue_timing: &'static str,
    segmentation_strategy: &'static str,
    window_count: usize,
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

#[derive(Debug, Clone, Copy)]
struct ChunkTiming {
    offset_cs: i64,
    duration_cs: i64,
}

#[derive(Debug, Clone, Copy)]
struct WordCueOptions {
    policy: CuePolicy,
    pause_split_cs: i64,
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
            "language={} decoding={} segmentation={} windows={} speech_regions={} speech_threshold={} speech_coverage={:.1}% cue_timing={} timed_words={} total_segments={} non_empty_segments={} blank_segments={}",
            self.language,
            self.decoding_strategy,
            self.segmentation_strategy,
            self.window_count,
            self.speech_region_count,
            self.speech_threshold
                .map(|threshold| format!("{threshold:.5}"))
                .unwrap_or_else(|| "n/a".to_string()),
            self.speech_coverage * 100.0,
            self.cue_timing,
            self.timed_word_count,
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
    engine: Option<String>,
    model: Option<String>,
    cleanup: Option<String>,
) -> Result<TranscribeOutput> {
    if !input.exists() {
        bail!("input media does not exist: {}", input.display());
    }

    ensure_ffmpeg_available()?;
    let mut loaded = load_or_init_config(config_path)?;
    apply_transcribe_overrides(&mut loaded.config.transcribe, engine, model, cleanup)?;
    ensure_dir(&loaded.paths.runtime_home)?;
    ensure_dir(&tmp_dir(&loaded.paths.runtime_home))?;

    let output = output.unwrap_or_else(|| default_output_path(&input));
    if paths_refer_to_same_file(&input, &output)? {
        bail!(
            "transcription output must be different from input media: {}",
            input.display()
        );
    }
    ensure_parent_dir(&output).with_context(|| {
        format!(
            "failed to create output directory for transcription {}",
            output.display()
        )
    })?;

    let (engine_label, model_path) = match loaded.config.transcribe.engine {
        TranscribeEngine::WhisperRs => {
            WHISPER_LOGGING_HOOKS.call_once(install_logging_hooks);
            (
                "whisper-rs",
                ensure_model_downloaded(&loaded.paths, &loaded.config)?,
            )
        }
        TranscribeEngine::FasterWhisper => (
            "faster-whisper",
            loaded
                .config
                .transcribe
                .model
                .path
                .clone()
                .unwrap_or_else(|| {
                    crate::runtime::models_dir(&loaded.paths.runtime_home).join("faster-whisper")
                }),
        ),
    };
    let audio_prefix = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("transcribe-audio");
    let audio = ScopedTempPath::file(
        &tmp_dir(&loaded.paths.runtime_home),
        audio_prefix,
        Some("wav"),
    );
    let language = resolve_language(lang.as_deref(), Some(&loaded.config.transcribe.language))?;
    extract_audio_to_wav(&input, audio.path())?;
    let DecodedTranscript {
        mut cues,
        language: effective_language,
        fallback_language_used,
        fallback_decoding_used,
        decoding_strategy,
        cue_timing,
        segmentation_strategy,
        window_count,
    } = match loaded.config.transcribe.engine {
        TranscribeEngine::WhisperRs => transcribe_to_cues(
            &model_path,
            audio.path(),
            language.as_deref(),
            &loaded.config.transcribe,
        )?,
        TranscribeEngine::FasterWhisper => transcribe_with_faster_whisper(
            &loaded.paths.runtime_home,
            audio.path(),
            language.as_deref(),
            &loaded.config.transcribe,
        )?,
    };
    let cleanup_status = if loaded.config.transcribe.cleanup.enabled {
        cues = clean_cues(
            &loaded.paths.runtime_home,
            &effective_language,
            &loaded.config.transcribe.cleanup,
            &cues,
        )?;
        format!("codex:{}", loaded.config.transcribe.cleanup.model)
    } else {
        "disabled".to_string()
    };
    let qa_warnings = validate_cues(&cues, cue_policy(&loaded.config.transcribe))?.warnings;
    for warning in &qa_warnings {
        eprintln!("transcribe_qa_warning = {warning}");
    }
    write_srt_file(&output, &cues)?;

    Ok(TranscribeOutput {
        input,
        output,
        model_path,
        engine: engine_label,
        model: loaded.config.transcribe.model.id.clone(),
        language: effective_language,
        fallback_language_used,
        fallback_decoding_used,
        decoding_strategy,
        cue_count: cues.len(),
        qa_warning_count: qa_warnings.len(),
        cue_timing,
        segmentation_strategy,
        window_count,
        cleanup: cleanup_status,
        status: "generated",
    })
}

fn apply_transcribe_overrides(
    config: &mut TranscribeConfig,
    engine: Option<String>,
    model: Option<String>,
    cleanup: Option<String>,
) -> Result<()> {
    if let Some(engine) = engine {
        config.engine = match engine.trim().to_lowercase().as_str() {
            "whisper-rs" | "whisper" => TranscribeEngine::WhisperRs,
            "faster-whisper" | "faster" => TranscribeEngine::FasterWhisper,
            _ => bail!(
                "unsupported transcription engine `{engine}`; use whisper-rs or faster-whisper"
            ),
        };
    }
    if model.is_none() && config.model.path.is_none() {
        match config.engine {
            TranscribeEngine::WhisperRs
                if !matches!(
                    config.model.id.as_str(),
                    "tiny"
                        | "base"
                        | "small"
                        | "medium"
                        | "tiny.en"
                        | "base.en"
                        | "small.en"
                        | "medium.en"
                ) =>
            {
                config.model.id = "base".to_string();
            }
            TranscribeEngine::FasterWhisper
                if !matches!(config.model.id.as_str(), "large-v3" | "large-v3-turbo") =>
            {
                config.model.id = "large-v3-turbo".to_string();
            }
            _ => {}
        }
    }
    if let Some(model) = model {
        config.model.id = model;
        config.model.path = None;
    }
    if let Some(cleanup_model) = cleanup {
        config.cleanup.enabled = true;
        if !cleanup_model.trim().is_empty() {
            config.cleanup.model = cleanup_model;
        }
    }
    validate_transcribe_config(config)
}

fn transcribe_with_faster_whisper(
    runtime_home: &Path,
    audio_path: &Path,
    language: Option<&str>,
    config: &TranscribeConfig,
) -> Result<DecodedTranscript> {
    let model = config
        .model
        .path
        .as_deref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| config.model.id.clone());
    let output = faster_whisper::transcribe(runtime_home, audio_path, &model, language)?;
    if output.segments.is_empty() {
        bail!("faster-whisper returned no subtitle segments");
    }
    let timed_cues = build_cues_from_timed_words(
        &output.words,
        cue_policy(config),
        milliseconds_to_centiseconds(config.cues.pause_split_ms),
    );
    let words_match = transcript_text_key(
        &output
            .words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<String>(),
    ) == transcript_text_key(
        &output
            .segments
            .iter()
            .map(|cue| cue.text.as_str())
            .collect::<String>(),
    );
    let (raw_cues, cue_timing) = if !timed_cues.is_empty() && words_match {
        (timed_cues, "word")
    } else {
        (output.segments, "segment-fallback")
    };
    let media_duration_cs = output
        .duration_cs
        .max(raw_cues.last().map_or(0, |cue| cue.end_cs));
    let cues = optimize_cues(raw_cues, media_duration_cs, cue_policy(config));
    validate_cues(&cues, cue_policy(config))?;
    eprintln!(
        "transcribe_segmentation = strategy=silero-vad windows=1 speech_duration={:.1}s audio_duration={:.1}s",
        output.duration_after_vad_cs as f64 / 100.0,
        output.duration_cs as f64 / 100.0
    );
    eprintln!(
        "transcribe_cue_timing = strategy={cue_timing} timed_words={} word_timed_windows={} fallback_windows={}",
        output.words.len(),
        usize::from(cue_timing == "word"),
        usize::from(cue_timing != "word")
    );
    Ok(DecodedTranscript {
        cues,
        language: output.language,
        fallback_language_used: false,
        fallback_decoding_used: false,
        decoding_strategy: "beam",
        cue_timing,
        segmentation_strategy: "silero-vad",
        window_count: 1,
    })
}

fn default_output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("transcript");
    input.with_file_name(format!("{stem}.srt"))
}

fn transcribe_to_cues(
    model_path: &Path,
    audio_path: &Path,
    language: Option<&str>,
    config: &TranscribeConfig,
) -> Result<DecodedTranscript> {
    let reader = hound::WavReader::open(audio_path)
        .with_context(|| format!("failed to open extracted wav {}", audio_path.display()))?;
    let sample_rate = reader.spec().sample_rate as usize;
    let samples: Vec<i16> = reader
        .into_samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read wav samples from {}", audio_path.display()))?;
    if sample_rate == 0 {
        bail!(
            "extracted wav has invalid sample rate: {}",
            audio_path.display()
        );
    }

    let mut audio = vec![0.0f32; samples.len()];
    whisper_rs::convert_integer_to_float_audio(&samples, &mut audio)
        .context("failed to convert wav samples to whisper input format")?;
    if audio.is_empty() {
        bail!(
            "extracted wav contains no audio samples: {}",
            audio_path.display()
        );
    }
    if audio.len() > sample_rate.saturating_mul(60) {
        eprintln!(
            "transcribe_audio_duration_seconds = {:.1}",
            audio.len() as f64 / sample_rate as f64
        );
    }

    let ctx = WhisperContext::new_with_params(
        model_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("model path is not valid UTF-8"))?,
        WhisperContextParameters::default(),
    )
    .with_context(|| format!("failed to load whisper model {}", model_path.display()))?;

    let detected_language = match language {
        Some(_) => None,
        None => Some(detect_language(&ctx, detection_audio(&audio, sample_rate))?),
    };
    let attempts = build_attempts(language, detected_language.as_deref());
    let mut diagnostics = Vec::new();
    for attempt in attempts {
        let (cues, attempt_diagnostics) = transcribe_attempt(
            &ctx,
            &audio,
            sample_rate,
            Some(attempt.language.as_str()),
            attempt.strategy,
            config,
        )?;
        if !cues.is_empty() {
            validate_cues(&cues, cue_policy(config))?;
            return Ok(DecodedTranscript {
                cues,
                language: attempt.language,
                fallback_language_used: attempt.fallback_language_used,
                fallback_decoding_used: attempt.fallback_decoding_used,
                decoding_strategy: attempt.strategy.label(),
                cue_timing: attempt_diagnostics.cue_timing,
                segmentation_strategy: attempt_diagnostics.segmentation_strategy,
                window_count: attempt_diagnostics.window_count,
            });
        }
        diagnostics.push(attempt_diagnostics);
    }

    if let Some(requested_language) = language {
        let detected_language = detect_language(&ctx, detection_audio(&audio, sample_rate)).ok();
        if let Some(detected_language) = detected_language.as_deref()
            && detected_language != requested_language
        {
            for attempt in detected_language_attempts(detected_language) {
                let (cues, attempt_diagnostics) = transcribe_attempt(
                    &ctx,
                    &audio,
                    sample_rate,
                    Some(attempt.language.as_str()),
                    attempt.strategy,
                    config,
                )?;
                if !cues.is_empty() {
                    validate_cues(&cues, cue_policy(config))?;
                    return Ok(DecodedTranscript {
                        cues,
                        language: attempt.language,
                        fallback_language_used: attempt.fallback_language_used,
                        fallback_decoding_used: attempt.fallback_decoding_used,
                        decoding_strategy: attempt.strategy.label(),
                        cue_timing: attempt_diagnostics.cue_timing,
                        segmentation_strategy: attempt_diagnostics.segmentation_strategy,
                        window_count: attempt_diagnostics.window_count,
                    });
                }
                diagnostics.push(attempt_diagnostics);
            }
        }
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
        && detected_language != requested_language
    {
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

    attempts
}

fn detected_language_attempts(detected_language: &str) -> Vec<AttemptPlan> {
    vec![
        AttemptPlan::new(detected_language, true, false, DecodeStrategy::BeamSearch),
        AttemptPlan::new(detected_language, true, true, DecodeStrategy::Greedy),
    ]
}

fn detection_audio(audio: &[f32], sample_rate: usize) -> &[f32] {
    let sample_count = sample_rate.saturating_mul(LANGUAGE_DETECTION_SECONDS);
    let end = audio.len().min(sample_count);
    &audio[..end]
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

fn transcribe_attempt(
    ctx: &WhisperContext,
    audio: &[f32],
    sample_rate: usize,
    language: Option<&str>,
    strategy: DecodeStrategy,
    config: &TranscribeConfig,
) -> Result<(Vec<SubtitleCue>, AttemptDiagnostics)> {
    let window_plan = transcription_windows(audio, sample_rate, config)?;
    let chunks = window_plan.windows;
    eprintln!(
        "transcribe_segmentation = strategy={} windows={} speech_regions={} speech_threshold={} speech_coverage={:.1}%",
        window_plan.strategy,
        chunks.len(),
        window_plan.speech_region_count,
        window_plan
            .speech_threshold
            .map(|threshold| format!("{threshold:.5}"))
            .unwrap_or_else(|| "n/a".to_string()),
        window_plan.speech_coverage * 100.0
    );
    let mut cues_with_sources = Vec::new();
    let mut segment_count = 0usize;
    let mut non_empty_segments = 0usize;
    let mut timed_word_count = 0usize;
    let mut word_timed_windows = 0usize;

    for (index, chunk) in chunks.iter().enumerate() {
        if chunks.len() > 1 {
            eprintln!(
                "transcribe_progress = chunk {}/{} language={} decoding={} start_seconds={:.1} end_seconds={:.1}",
                index + 1,
                chunks.len(),
                language.unwrap_or("auto"),
                strategy.label(),
                chunk.start as f64 / sample_rate as f64,
                chunk.end as f64 / sample_rate as f64
            );
        }

        let timing = ChunkTiming {
            offset_cs: samples_to_centiseconds(chunk.start, sample_rate),
            duration_cs: samples_to_centiseconds(chunk.len(), sample_rate),
        };
        let (mut current_cues, chunk_diagnostics) = transcribe_chunk(
            ctx,
            &audio[chunk.clone()],
            language,
            strategy,
            timing,
            WordCueOptions {
                policy: cue_policy(config),
                pause_split_cs: milliseconds_to_centiseconds(config.cues.pause_split_ms),
            },
        )?;
        segment_count += chunk_diagnostics.segment_count;
        non_empty_segments += chunk_diagnostics.non_empty_segments;
        timed_word_count += chunk_diagnostics.timed_word_count;
        if chunk_diagnostics.cue_timing == "word" {
            word_timed_windows += 1;
        }
        current_cues.retain_mut(|cue| constrain_cue_to_chunk(cue, index, &chunks, sample_rate));
        append_boundary_cues(&mut cues_with_sources, current_cues, index);
    }

    let media_duration_cs = samples_to_centiseconds(audio.len(), sample_rate);
    let cues = optimize_cues(
        cues_with_sources.into_iter().map(|(_, cue)| cue).collect(),
        media_duration_cs,
        cue_policy(config),
    );
    let cue_timing = match (word_timed_windows, chunks.len()) {
        (0, _) => "segment-fallback",
        (word_timed, total) if word_timed == total => "word",
        _ => "mixed",
    };
    eprintln!(
        "transcribe_cue_timing = strategy={cue_timing} timed_words={timed_word_count} word_timed_windows={word_timed_windows} fallback_windows={}",
        chunks.len().saturating_sub(word_timed_windows)
    );

    Ok((
        cues,
        AttemptDiagnostics {
            language: language.unwrap_or("auto").to_string(),
            decoding_strategy: strategy.label(),
            segmentation_strategy: window_plan.strategy,
            window_count: chunks.len(),
            speech_region_count: window_plan.speech_region_count,
            speech_threshold: window_plan.speech_threshold,
            speech_coverage: window_plan.speech_coverage,
            cue_timing,
            timed_word_count,
            segment_count,
            non_empty_segments,
        },
    ))
}

fn transcribe_chunk(
    ctx: &WhisperContext,
    audio: &[f32],
    language: Option<&str>,
    strategy: DecodeStrategy,
    timing: ChunkTiming,
    cue_options: WordCueOptions,
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
    params.set_token_timestamps(true);
    params.set_translate(false);
    params.set_language(language);
    // `detect_language=true` makes whisper.cpp return after detection without transcribing.
    // For actual transcription we always pass an explicit language, including auto-detected ones.
    params.set_detect_language(false);
    params.set_n_threads(num_cpus::get_physical().max(1) as i32);

    state
        .full(params, audio)
        .context("failed to transcribe audio with whisper")?;

    collect_cues(
        &state,
        language,
        strategy,
        timing,
        ctx.token_eot(),
        cue_options,
    )
}

#[derive(Debug)]
struct TranscriptionWindowPlan {
    windows: Vec<Range<usize>>,
    strategy: &'static str,
    speech_region_count: usize,
    speech_threshold: Option<f32>,
    speech_coverage: f64,
}

fn transcription_windows(
    audio: &[f32],
    sample_rate: usize,
    config: &TranscribeConfig,
) -> Result<TranscriptionWindowPlan> {
    let segmentation = &config.segmentation;
    let overlap_seconds = usize::try_from(segmentation.overlap_seconds)
        .context("transcribe overlap duration is too large for this platform")?;

    if segmentation.mode == TranscribeSegmentationMode::Speech {
        let options = SpeechWindowOptions {
            min_speech_ms: usize::try_from(segmentation.min_speech_ms)
                .context("minimum speech duration is too large for this platform")?,
            min_silence_ms: usize::try_from(segmentation.min_silence_ms)
                .context("minimum silence duration is too large for this platform")?,
            padding_ms: usize::try_from(segmentation.padding_ms)
                .context("speech padding is too large for this platform")?,
            max_window_seconds: usize::try_from(segmentation.max_window_seconds)
                .context("maximum speech window is too large for this platform")?,
            overlap_seconds,
        };
        let speech_plan = speech_windows(audio, sample_rate, options);
        let speech_coverage = speech_plan.speech_sample_count as f64 / audio.len().max(1) as f64;
        if !speech_plan.windows.is_empty() {
            return Ok(TranscriptionWindowPlan {
                windows: speech_plan.windows,
                strategy: "speech",
                speech_region_count: speech_plan.speech_region_count,
                speech_threshold: Some(speech_plan.threshold),
                speech_coverage,
            });
        }

        let chunk_seconds = usize::try_from(segmentation.chunk_seconds)
            .context("transcribe chunk duration is too large for this platform")?;
        return Ok(TranscriptionWindowPlan {
            windows: fixed_windows(audio.len(), sample_rate, chunk_seconds, overlap_seconds),
            strategy: "fixed-fallback",
            speech_region_count: 0,
            speech_threshold: Some(speech_plan.threshold),
            speech_coverage,
        });
    }

    let chunk_seconds = usize::try_from(segmentation.chunk_seconds)
        .context("transcribe chunk duration is too large for this platform")?;
    Ok(TranscriptionWindowPlan {
        windows: fixed_windows(audio.len(), sample_rate, chunk_seconds, overlap_seconds),
        strategy: "fixed",
        speech_region_count: 0,
        speech_threshold: None,
        speech_coverage: 0.0,
    })
}

fn cue_belongs_to_chunk(
    cue: &SubtitleCue,
    chunk_index: usize,
    chunks: &[Range<usize>],
    sample_rate: usize,
) -> bool {
    let (lower_cs, upper_cs) = chunk_ownership_bounds(chunk_index, chunks, sample_rate);
    let midpoint_cs = midpoint_i64(cue.start_cs, cue.end_cs);

    midpoint_cs >= lower_cs && (chunk_index + 1 == chunks.len() || midpoint_cs < upper_cs)
}

fn constrain_cue_to_chunk(
    cue: &mut SubtitleCue,
    chunk_index: usize,
    chunks: &[Range<usize>],
    sample_rate: usize,
) -> bool {
    if !cue_belongs_to_chunk(cue, chunk_index, chunks, sample_rate) {
        return false;
    }

    let (lower_cs, upper_cs) = chunk_ownership_bounds(chunk_index, chunks, sample_rate);
    cue.start_cs = cue.start_cs.max(lower_cs);
    cue.end_cs = cue.end_cs.min(upper_cs);
    cue.end_cs > cue.start_cs
}

fn chunk_ownership_bounds(
    chunk_index: usize,
    chunks: &[Range<usize>],
    sample_rate: usize,
) -> (i64, i64) {
    let lower_sample = if chunk_index == 0 {
        chunks[chunk_index].start
    } else {
        midpoint(chunks[chunk_index - 1].end, chunks[chunk_index].start)
    };
    let upper_sample = if chunk_index + 1 == chunks.len() {
        chunks[chunk_index].end
    } else {
        midpoint(chunks[chunk_index].end, chunks[chunk_index + 1].start)
    };
    let lower_cs = samples_to_centiseconds(lower_sample, sample_rate);
    let upper_cs = samples_to_centiseconds(upper_sample, sample_rate);
    (lower_cs, upper_cs)
}

fn midpoint(left: usize, right: usize) -> usize {
    left.min(right).saturating_add(left.abs_diff(right) / 2)
}

fn midpoint_i64(left: i64, right: i64) -> i64 {
    left.min(right)
        .saturating_add(left.abs_diff(right) as i64 / 2)
}

fn append_boundary_cues(
    target: &mut Vec<(usize, SubtitleCue)>,
    source: Vec<SubtitleCue>,
    chunk_index: usize,
) {
    for cue in source {
        let cue_key = boundary_text_key(&cue.text);
        if let Some((previous_chunk, previous)) = target.last_mut()
            && *previous_chunk != chunk_index
            && !cue_key.is_empty()
            && boundary_texts_match(&boundary_text_key(&previous.text), &cue_key)
            && cue.start_cs <= previous.end_cs.saturating_add(25)
        {
            previous.start_cs = previous.start_cs.min(cue.start_cs);
            previous.end_cs = previous.end_cs.max(cue.end_cs);
            if cue.text.chars().count() > previous.text.chars().count() {
                previous.text = cue.text;
            }
            continue;
        }
        target.push((chunk_index, cue));
    }
}

fn boundary_texts_match(previous: &str, current: &str) -> bool {
    previous == current
        || (previous.chars().count().min(current.chars().count()) >= 4
            && (previous.contains(current) || current.contains(previous)))
}

fn boundary_text_key(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn cue_policy(config: &TranscribeConfig) -> CuePolicy {
    CuePolicy {
        min_duration_cs: milliseconds_to_centiseconds(config.cues.min_duration_ms),
        max_duration_cs: milliseconds_to_centiseconds(config.cues.max_duration_ms),
        end_padding_cs: milliseconds_to_centiseconds(config.cues.end_padding_ms),
        max_chars_per_line: config.cues.max_chars_per_line,
        max_cjk_chars_per_line: config.cues.max_cjk_chars_per_line,
        max_lines: config.cues.max_lines,
    }
}

fn milliseconds_to_centiseconds(milliseconds: u64) -> i64 {
    let centiseconds = milliseconds.saturating_add(9) / 10;
    centiseconds.min(i64::MAX as u64) as i64
}

fn samples_to_centiseconds(sample_count: usize, sample_rate: usize) -> i64 {
    ((sample_count as u64).saturating_mul(100) / sample_rate.max(1) as u64) as i64
}

fn collect_cues(
    state: &whisper_rs::WhisperState,
    language: Option<&str>,
    strategy: DecodeStrategy,
    timing: ChunkTiming,
    token_eot: i32,
    cue_options: WordCueOptions,
) -> Result<(Vec<SubtitleCue>, AttemptDiagnostics)> {
    let mut segment_cues = Vec::new();
    let mut timed_words = Vec::new();
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

        if let Some((start_cs, end_cs)) = clamp_segment_to_chunk(
            segment.start_timestamp(),
            segment.end_timestamp(),
            timing.offset_cs,
            timing.duration_cs,
        ) {
            segment_cues.push(SubtitleCue {
                index: index + 1,
                start_cs,
                end_cs,
                text,
            });
        }

        collect_segment_timed_words(
            &segment,
            token_eot,
            timing.offset_cs,
            timing.duration_cs,
            &mut timed_words,
        )?;
    }

    let timed_cues =
        build_cues_from_timed_words(&timed_words, cue_options.policy, cue_options.pause_split_cs);
    let word_text_matches = transcript_text_key(
        &timed_words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<String>(),
    ) == transcript_text_key(
        &segment_cues
            .iter()
            .map(|cue| cue.text.as_str())
            .collect::<String>(),
    );
    let non_empty_segments = segment_cues.len();
    let (cues, cue_timing) = if !timed_cues.is_empty() && word_text_matches {
        (timed_cues, "word")
    } else {
        (segment_cues, "segment-fallback")
    };
    Ok((
        cues,
        AttemptDiagnostics {
            language: language.unwrap_or("auto").to_string(),
            decoding_strategy: strategy.label(),
            segmentation_strategy: "window",
            window_count: 1,
            speech_region_count: 0,
            speech_threshold: None,
            speech_coverage: 0.0,
            cue_timing,
            timed_word_count: timed_words.len(),
            segment_count,
            non_empty_segments,
        },
    ))
}

fn collect_segment_timed_words(
    segment: &whisper_rs::WhisperSegment<'_>,
    token_eot: i32,
    chunk_offset_cs: i64,
    chunk_duration_cs: i64,
    target: &mut Vec<TimedWord>,
) -> Result<()> {
    let mut pending_bytes = Vec::new();
    let mut pending_start_cs = None;
    let mut pending_end_cs = None;

    for token_index in 0..segment.n_tokens() {
        let Some(token) = segment.get_token(token_index) else {
            continue;
        };
        if token.token_id() >= token_eot {
            continue;
        }
        let token_data = token.token_data();
        let Some((start_cs, end_cs)) = clamp_token_to_chunk(
            token_data.t0,
            token_data.t1,
            chunk_offset_cs,
            chunk_duration_cs,
        ) else {
            continue;
        };
        let bytes = token
            .to_bytes()
            .context("failed to decode whisper token text")?;
        if bytes.is_empty() {
            continue;
        }

        pending_bytes.extend_from_slice(bytes);
        pending_start_cs.get_or_insert(start_cs);
        pending_end_cs = Some(pending_end_cs.map_or(end_cs, |end: i64| end.max(end_cs)));
        match std::str::from_utf8(&pending_bytes) {
            Ok(text) => {
                if !text.is_empty() {
                    target.push(TimedWord {
                        start_cs: pending_start_cs.unwrap_or(start_cs),
                        end_cs: pending_end_cs.unwrap_or(end_cs),
                        text: text.to_string(),
                    });
                }
                pending_bytes.clear();
                pending_start_cs = None;
                pending_end_cs = None;
            }
            Err(error) if error.error_len().is_none() => {}
            Err(_) => {
                pending_bytes.clear();
                pending_start_cs = None;
                pending_end_cs = None;
            }
        }
    }

    Ok(())
}

fn transcript_text_key(text: &str) -> String {
    let mut key = String::new();
    let mut pending_space = false;
    let mut previous_was_ascii_alphanumeric = false;

    for character in text.chars() {
        if character.is_whitespace() {
            pending_space = true;
            continue;
        }
        if !character.is_alphanumeric() {
            continue;
        }

        let current_is_ascii_alphanumeric = character.is_ascii_alphanumeric();
        if pending_space && previous_was_ascii_alphanumeric && current_is_ascii_alphanumeric {
            key.push(' ');
        }
        key.extend(character.to_lowercase());
        pending_space = false;
        previous_was_ascii_alphanumeric = current_is_ascii_alphanumeric;
    }

    key
}

fn clamp_token_to_chunk(
    token_start_cs: i64,
    token_end_cs: i64,
    chunk_offset_cs: i64,
    chunk_duration_cs: i64,
) -> Option<(i64, i64)> {
    let chunk_end_cs = chunk_offset_cs.saturating_add(chunk_duration_cs.max(0));
    let start_cs = chunk_offset_cs
        .saturating_add(token_start_cs)
        .clamp(chunk_offset_cs, chunk_end_cs);
    if start_cs >= chunk_end_cs {
        return None;
    }
    let end_cs = chunk_offset_cs
        .saturating_add(token_end_cs)
        .clamp(chunk_offset_cs, chunk_end_cs)
        .max(start_cs.saturating_add(1).min(chunk_end_cs));
    (end_cs > start_cs).then_some((start_cs, end_cs))
}

fn clamp_segment_to_chunk(
    segment_start_cs: i64,
    segment_end_cs: i64,
    chunk_offset_cs: i64,
    chunk_duration_cs: i64,
) -> Option<(i64, i64)> {
    let chunk_end_cs = chunk_offset_cs.saturating_add(chunk_duration_cs.max(0));
    let start_cs = chunk_offset_cs
        .saturating_add(segment_start_cs)
        .clamp(chunk_offset_cs, chunk_end_cs);
    let end_cs = chunk_offset_cs
        .saturating_add(segment_end_cs)
        .clamp(chunk_offset_cs, chunk_end_cs);
    (end_cs > start_cs).then_some((start_cs, end_cs))
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
        AttemptDiagnostics, AttemptPlan, DecodeStrategy, append_boundary_cues,
        apply_transcribe_overrides, build_attempts, clamp_segment_to_chunk, clamp_token_to_chunk,
        constrain_cue_to_chunk, cue_belongs_to_chunk, detected_language_attempts, detection_audio,
        resolve_language, samples_to_centiseconds, transcript_text_key, transcription_windows,
    };
    use crate::config::{TranscribeEngine, TranscribeSegmentationMode, default_config};
    use crate::segmentation::fixed_windows;
    use crate::subtitles::SubtitleCue;

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
    fn engine_override_selects_a_compatible_default_model() {
        let mut config = default_config()
            .expect("default config should build")
            .transcribe;
        apply_transcribe_overrides(&mut config, Some("faster-whisper".to_string()), None, None)
            .expect("faster-whisper override should be valid");

        assert_eq!(config.engine, TranscribeEngine::FasterWhisper);
        assert_eq!(config.model.id, "large-v3-turbo");

        apply_transcribe_overrides(&mut config, Some("whisper-rs".to_string()), None, None)
            .expect("whisper-rs override should be valid");
        assert_eq!(config.engine, TranscribeEngine::WhisperRs);
        assert_eq!(config.model.id, "base");
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
            segmentation_strategy: "speech",
            window_count: 2,
            speech_region_count: 2,
            speech_threshold: Some(0.01),
            speech_coverage: 0.75,
            cue_timing: "word",
            timed_word_count: 12,
            segment_count: 4,
            non_empty_segments: 1,
        };
        let summary = diagnostics.summary();
        assert!(summary.contains("language=zh"));
        assert!(summary.contains("decoding=beam"));
        assert!(summary.contains("segmentation=speech"));
        assert!(summary.contains("windows=2"));
        assert!(summary.contains("speech_coverage=75.0%"));
        assert!(summary.contains("cue_timing=word"));
        assert!(summary.contains("timed_words=12"));
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

    #[test]
    fn explicit_language_attempts_do_not_require_detection_first() {
        let attempts = build_attempts(Some("ja"), None);

        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0],
            AttemptPlan::new("ja", false, false, DecodeStrategy::BeamSearch)
        );
        assert_eq!(
            attempts[1],
            AttemptPlan::new("ja", false, true, DecodeStrategy::Greedy)
        );
    }

    #[test]
    fn detected_language_fallback_attempts_are_marked() {
        let attempts = detected_language_attempts("zh");

        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0],
            AttemptPlan::new("zh", true, false, DecodeStrategy::BeamSearch)
        );
        assert_eq!(
            attempts[1],
            AttemptPlan::new("zh", true, true, DecodeStrategy::Greedy)
        );
    }

    #[test]
    fn detection_audio_uses_initial_window() {
        let audio = vec![0.0; 16_000 * 40];
        let window = detection_audio(&audio, 16_000);

        assert_eq!(window.len(), 16_000 * 30);
    }

    #[test]
    fn fixed_windows_bound_long_input() {
        let chunks = fixed_windows(16_000 * 75, 16_000, 30, 2);

        assert_eq!(
            chunks,
            vec![0..480_000, 448_000..928_000, 896_000..1_200_000]
        );
    }

    #[test]
    fn speech_segmentation_falls_back_for_undetected_audio() {
        let config = default_config().expect("default config should build");
        let plan = transcription_windows(&vec![0.0; 7_500], 100, &config.transcribe)
            .expect("window plan should build");

        assert_eq!(plan.strategy, "fixed-fallback");
        assert_eq!(plan.windows, vec![0..3_000, 2_800..5_800, 5_600..7_500]);
    }

    #[test]
    fn speech_and_fixed_segmentation_modes_are_explicit() {
        let mut config = default_config().expect("default config should build");
        let mut audio = vec![0.0; 1_000];
        audio[200..400].fill(0.1);

        let speech_plan = transcription_windows(&audio, 100, &config.transcribe)
            .expect("speech window plan should build");
        assert_eq!(speech_plan.strategy, "speech");
        assert_eq!(speech_plan.windows, vec![170..430]);

        config.transcribe.segmentation.mode = TranscribeSegmentationMode::Fixed;
        let fixed_plan = transcription_windows(&audio, 100, &config.transcribe)
            .expect("fixed window plan should build");
        assert_eq!(fixed_plan.strategy, "fixed");
        assert_eq!(fixed_plan.windows, vec![0..1_000]);
    }

    #[test]
    fn overlap_ownership_assigns_boundary_to_one_chunk() {
        let chunks = fixed_windows(75, 1, 30, 2);
        let boundary_cue = SubtitleCue {
            index: 1,
            start_cs: 2_850,
            end_cs: 2_950,
            text: "boundary".to_string(),
        };

        assert!(!cue_belongs_to_chunk(&boundary_cue, 0, &chunks, 1));
        assert!(cue_belongs_to_chunk(&boundary_cue, 1, &chunks, 1));

        let mut clipped = SubtitleCue {
            index: 1,
            start_cs: 2_800,
            end_cs: 3_400,
            text: "owned by the second chunk".to_string(),
        };
        assert!(constrain_cue_to_chunk(&mut clipped, 1, &chunks, 1));
        assert_eq!(clipped.start_cs, 2_900);
        assert_eq!(clipped.end_cs, 3_400);
    }

    #[test]
    fn repeated_boundary_cues_are_deduplicated() {
        let mut cues = vec![(
            0,
            SubtitleCue {
                index: 1,
                start_cs: 2_800,
                end_cs: 2_920,
                text: "Hello".to_string(),
            },
        )];
        append_boundary_cues(
            &mut cues,
            vec![SubtitleCue {
                index: 1,
                start_cs: 2_900,
                end_cs: 3_000,
                text: "hello!".to_string(),
            }],
            1,
        );

        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].1.start_cs, 2_800);
        assert_eq!(cues[0].1.end_cs, 3_000);
    }

    #[test]
    fn contained_boundary_suffix_is_deduplicated() {
        let mut cues = vec![(
            3,
            SubtitleCue {
                index: 1,
                start_cs: 10_700,
                end_cs: 11_300,
                text: "只有在异常输入中保持稳定才真正适合日常使用".to_string(),
            },
        )];
        append_boundary_cues(
            &mut cues,
            vec![SubtitleCue {
                index: 1,
                start_cs: 11_300,
                end_cs: 11_500,
                text: "才真正适合日常使用".to_string(),
            }],
            4,
        );

        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].1.end_cs, 11_500);
        assert!(cues[0].1.text.starts_with("只有"));
    }

    #[test]
    fn samples_convert_to_centiseconds() {
        assert_eq!(samples_to_centiseconds(16_000, 16_000), 100);
        assert_eq!(samples_to_centiseconds(24_000, 16_000), 150);
    }

    #[test]
    fn segment_timestamps_are_clamped_to_chunk_boundaries() {
        assert_eq!(
            clamp_segment_to_chunk(2_900, 3_100, 3_000, 3_000),
            Some((5_900, 6_000))
        );
        assert_eq!(clamp_segment_to_chunk(3_000, 3_100, 3_000, 3_000), None);
        assert_eq!(
            clamp_segment_to_chunk(-100, 200, 3_000, 3_000),
            Some((3_000, 3_200))
        );
    }

    #[test]
    fn token_timestamps_gain_chunk_offset_and_minimum_duration() {
        assert_eq!(
            clamp_token_to_chunk(10, 20, 3_000, 500),
            Some((3_010, 3_020))
        );
        assert_eq!(
            clamp_token_to_chunk(30, 30, 3_000, 500),
            Some((3_030, 3_031))
        );
        assert_eq!(clamp_token_to_chunk(500, 510, 3_000, 500), None);
    }

    #[test]
    fn timed_token_integrity_preserves_latin_word_spacing() {
        assert_eq!(transcript_text_key(" Hello   world. "), "hello world");
        assert_eq!(transcript_text_key("Hello, world."), "hello world");
        assert_ne!(
            transcript_text_key("Hello world"),
            transcript_text_key("Helloworld")
        );
    }
}
