//! Durable caption projects. Agent output is a reviewable draft; rendering is an
//! explicit operation bound to the exact captions and assets that were accepted.
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::caption_agent::{self, ReviewReport, ReviewedCaptions};
use crate::cli::{WorkflowRenderArgs, WorkflowStartArgs};
use crate::config::{Config, TranscribeCueConfig, load_config};
use crate::pipelines::{burnin, transcribe};
use crate::render::burn_in_subtitles_with_watermark;
use crate::runtime::{ScopedTempPath, atomic_write, ensure_parent_dir, parent_dir};
use crate::subtitles::{SrtCue, parse_srt_file, write_srt_file};
use crate::verification::{probe_video, verify_rendered_video};
use crate::watermark_presets;

const FORMAT_VERSION: u32 = 1;
const MANIFEST: &str = "project.json";
const CONFIG: &str = "config.toml";
const RAW: &str = "raw.srt";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Project {
    format_version: u32,
    created_with: String,
    media: PathBuf,
    media_sha256: String,
    duration_seconds: f64,
    target_language: Option<String>,
    context: String,
    watermark: Option<String>,
    raw_sha256: Option<String>,
    source: Option<Stage>,
    target: Option<Stage>,
    acceptance: Option<Acceptance>,
    renders: Vec<RenderRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stage {
    directory: String,
    input_sha256: String,
    generated_sha256: String,
    report_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewFingerprint {
    source_sha256: String,
    selected_sha256: String,
    source_report_sha256: String,
    target_report_sha256: Option<String>,
    watermark_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Acceptance {
    reviewed_at_unix_seconds: u64,
    note: String,
    issues_acknowledged: usize,
    manually_edited_source: bool,
    manually_edited_selected: bool,
    fingerprint: ReviewFingerprint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderRecord {
    output: PathBuf,
    sha256: String,
    verification: String,
    fingerprint: ReviewFingerprint,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowStatus {
    #[serde(skip)]
    fingerprint: Option<ReviewFingerprint>,
    pub project: PathBuf,
    pub state: String,
    pub media: PathBuf,
    pub raw_subtitles: Option<PathBuf>,
    pub source_subtitles: Option<PathBuf>,
    pub selected_subtitles: Option<PathBuf>,
    pub review_file: PathBuf,
    pub issues: Vec<WorkflowIssue>,
    pub manually_edited_source: bool,
    pub manually_edited_selected: bool,
    pub next_action: String,
    pub last_output: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowIssue {
    pub stage: String,
    pub cue_id: usize,
    pub kind: String,
    pub message: String,
}

pub fn start(args: WorkflowStartArgs) -> Result<WorkflowStatus> {
    let media = fs::canonicalize(&args.video)
        .with_context(|| format!("cannot open video {}", args.video.display()))?;
    let metadata = probe_video(&media)?;
    let mut config = load_config(args.config_path)?.config;
    transcribe::apply_transcribe_overrides(&mut config.transcribe, args.engine, args.model, None)?;
    if let Some(language) = args.lang {
        config.transcribe.language = language;
    }
    // The project retains the raw ASR output before its mandatory agent pass.
    config.transcribe.cleanup.enabled = false;
    let target_language = args.to.map(|value| value.trim().to_owned());
    if target_language.as_ref().is_some_and(String::is_empty) {
        bail!("target language must not be empty");
    }
    let context = match args.context_file {
        Some(path) => read_text_bounded(&path, 64 * 1024)?,
        None => String::new(),
    };
    if context.chars().count() > caption_agent::MAX_CONTEXT_CHARACTERS {
        bail!("caption context is too long; use a glossary or summary under 32,000 characters");
    }
    crate::translate::validate_agent_config(
        &config.transcribe.cleanup.command,
        &config.transcribe.cleanup.model,
        &config.transcribe.cleanup.reasoning_effort,
        "transcribe.cleanup",
    )?;
    if target_language.is_some() {
        crate::translate::validate_config(&config.translate)?;
    }
    let imported = match args.subs {
        Some(path) => {
            let cues = parse_srt_file(&path)?;
            check_cues(
                &cues,
                metadata.duration_seconds,
                &config.transcribe.cues,
                "source",
            )?;
            Some(cues)
        }
        None => None,
    };
    let watermark = args
        .watermark
        .as_ref()
        .map(|name| watermark_presets::load(&config.runtime.home, name))
        .transpose()?;
    let path = args.project.unwrap_or_else(|| {
        media.with_file_name(format!(
            "{}.lilaccaps",
            media
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("video")
        ))
    });
    ensure_parent_dir(&path)?;
    fs::create_dir(&path)
        .with_context(|| format!("project directory must be new: {}", path.display()))?;
    let path = fs::canonicalize(path)?;
    let _lock = lock_project(&path)?;
    let project = Project {
        format_version: FORMAT_VERSION,
        created_with: env!("CARGO_PKG_VERSION").to_owned(),
        media_sha256: file_sha256(&media)?,
        media,
        duration_seconds: metadata.duration_seconds,
        target_language,
        context,
        watermark: args.watermark,
        raw_sha256: None,
        source: None,
        target: None,
        acceptance: None,
        renders: Vec::new(),
    };
    if let Some(preset) = watermark {
        watermark_presets::snapshot(&preset, &path.join("watermark"))?;
    }
    atomic_write(&path.join(CONFIG), toml::to_string_pretty(&config)?)?;
    save_project(&path, &project)?;
    if let Some(cues) = imported {
        write_srt_file(&path.join(RAW), &cues)?;
    }
    resume_project(&path, false, false).with_context(|| {
        format!(
            "project retained at {}; use `lilaccaps workflow resume` with this path to continue",
            path.display()
        )
    })
}

pub fn resume(path: &Path, review_source: bool, retranslate: bool) -> Result<WorkflowStatus> {
    let path = project_path(path)?;
    let _lock = lock_project(&path)?;
    resume_project(&path, review_source, retranslate)
}

fn resume_project(path: &Path, review_source: bool, retranslate: bool) -> Result<WorkflowStatus> {
    let mut project = load_project(path)?;
    if retranslate && project.target_language.is_none() {
        bail!("this project has no target language to translate");
    }
    verify_media(&project)?;
    let config = project_config(path)?;
    if project.raw_sha256.is_none() {
        if !path.join(RAW).try_exists()? {
            eprintln!("workflow_stage = transcribe");
            transcribe::run(
                project.media.clone(),
                Some(path.join(CONFIG)),
                Some(path.join(RAW)),
                None,
                None,
                None,
                None,
            )?;
        }
        regular_file(&path.join(RAW))?;
        check_cues(
            &parse_srt_file(&path.join(RAW))?,
            project.duration_seconds,
            &config.transcribe.cues,
            "raw",
        )?;
        project.raw_sha256 = Some(file_sha256(&path.join(RAW))?);
        save_project(path, &project)?;
    }
    verify_raw(path, &project)?;
    if project.source.is_none() || review_source {
        eprintln!("workflow_stage = review_source");
        let input = if let Some(stage) = &project.source {
            captions_path(path, stage, "source")?
        } else {
            path.join(RAW)
        };
        let input_hash = file_sha256(&input)?;
        let cues = parse_srt_file(&input)?;
        check_cues(
            &cues,
            project.duration_seconds,
            &config.transcribe.cues,
            "source",
        )?;
        let reviewed = caption_agent::review_transcript(
            &config.runtime.home,
            &config.transcribe.cleanup,
            &cues,
            &project.context,
        )?;
        check_cues(
            &reviewed.cues,
            project.duration_seconds,
            &config.transcribe.cues,
            "source",
        )?;
        project.source = Some(save_stage(path, "source", &input_hash, reviewed)?);
        project.acceptance = None;
        save_project(path, &project)?;
    }
    let source_stage = project.source.as_ref().expect("source checkpoint");
    let source_path = captions_path(path, source_stage, "source")?;
    let source_hash = file_sha256(&source_path)?;
    let source_cues = parse_srt_file(&source_path)?;
    check_cues(
        &source_cues,
        project.duration_seconds,
        &config.transcribe.cues,
        "source",
    )?;
    if let Some(language) = &project.target_language
        && (retranslate
            || project
                .target
                .as_ref()
                .is_none_or(|stage| stage.input_sha256 != source_hash))
    {
        eprintln!("workflow_stage = translate_and_review");
        let reviewed = caption_agent::translate_cues(
            &config.runtime.home,
            &config.translate,
            language,
            &source_cues,
            &project.context,
        )?;
        ensure_alignment(&source_cues, &reviewed.cues)?;
        check_cues(
            &reviewed.cues,
            project.duration_seconds,
            &config.transcribe.cues,
            "target",
        )?;
        // Every refresh gets a new directory. Older translations and user edits stay intact.
        project.target = Some(save_stage(path, "target", &source_hash, reviewed)?);
        project.acceptance = None;
        save_project(path, &project)?;
    }
    let status = status_for(path, &project)?;
    write_review(path, &project, &status)?;
    Ok(status)
}

pub fn status(path: &Path) -> Result<WorkflowStatus> {
    let path = project_path(path)?;
    let project = load_project(&path)?;
    verify_media(&project)?;
    status_for(&path, &project)
}

pub fn accept(path: &Path, note: &str) -> Result<WorkflowStatus> {
    if note.trim().is_empty() {
        bail!("review note must describe what was checked");
    }
    let path = project_path(path)?;
    let _lock = lock_project(&path)?;
    let mut project = load_project(&path)?;
    verify_media(&project)?;
    let current = status_for(&path, &project)?;
    if !matches!(
        current.state.as_str(),
        "review_required" | "accepted" | "rendered"
    ) {
        bail!("workflow is {}; {}", current.state, current.next_action);
    }
    let fingerprint = current
        .fingerprint
        .context("review fingerprint is not ready")?;
    if self::fingerprint(&path, &project)? != fingerprint {
        bail!("captions or watermark changed during acceptance; review and accept them again");
    }
    project.acceptance = Some(Acceptance {
        reviewed_at_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        note: note.trim().to_owned(),
        issues_acknowledged: current.issues.len(),
        manually_edited_source: current.manually_edited_source,
        manually_edited_selected: current.manually_edited_selected,
        fingerprint,
    });
    save_project(&path, &project)?;
    let status = status_for(&path, &project)?;
    write_review(&path, &project, &status)?;
    Ok(status)
}

pub fn render(args: WorkflowRenderArgs) -> Result<WorkflowStatus> {
    let path = project_path(&args.project)?;
    let _lock = lock_project(&path)?;
    let mut project = load_project(&path)?;
    verify_media(&project)?;
    let current = status_for(&path, &project)?;
    if !matches!(current.state.as_str(), "accepted" | "rendered") {
        bail!("workflow is {}; {}", current.state, current.next_action);
    }
    let expected = project
        .acceptance
        .as_ref()
        .context("captions have not been accepted")?
        .fingerprint
        .clone();
    if fingerprint(&path, &project)? != expected {
        bail!("captions or watermark changed after acceptance; review and accept them again");
    }
    let selected = current.selected_subtitles.context("no selected captions")?;
    let output = args.output.unwrap_or_else(|| path.join("final.mp4"));
    ensure_parent_dir(&output)?;
    let output = fs::canonicalize(parent_dir(&output))?.join(
        output
            .file_name()
            .context("output must name a video file")?,
    );
    if fs::symlink_metadata(&output).is_ok() {
        bail!(
            "output already exists; choose a new --output path: {}",
            output.display()
        );
    }
    let config = project_config(&path)?;
    let style = burnin::configured_style(&config, args.font, args.size);
    let preset = if project.watermark.is_some() {
        Some(watermark_presets::load_snapshot(&path.join("watermark"))?)
    } else {
        None
    };
    let temporary = ScopedTempPath::directory(parent_dir(&output), "workflow-render")?;
    let frozen_subs = temporary.path().join("captions.srt");
    fs::copy(&selected, &frozen_subs)?;
    if file_sha256(&frozen_subs)? != expected.selected_sha256 {
        bail!("captions changed while preparing the render; review and accept them again");
    }
    let preset = preset
        .as_ref()
        .map(|_| {
            let frozen = temporary.path().join("watermark");
            copy_directory(&path.join("watermark"), &frozen)?;
            watermark_presets::load_snapshot(&frozen)
        })
        .transpose()?;
    if preset.is_some()
        && Some(directory_sha256(&temporary.path().join("watermark"))?) != expected.watermark_sha256
    {
        bail!("watermark changed while preparing the render; review and accept it again");
    }
    let candidate = temporary.path().join(format!(
        "candidate.{}",
        output.extension().and_then(|s| s.to_str()).unwrap_or("mp4")
    ));
    eprintln!("workflow_stage = render");
    let renderer = burn_in_subtitles_with_watermark(
        &config.runtime.home,
        &project.media,
        &frozen_subs,
        &candidate,
        &style,
        preset.as_ref().map(|saved| (&saved.source, &saved.style)),
    )?;
    eprintln!("workflow_stage = verify_video");
    let verification = verify_rendered_video(&project.media, &candidate, &config.runtime.home)?;
    verify_media(&project)?;
    if fingerprint(&path, &project)? != expected {
        bail!("captions or watermark changed during rendering; review and accept them again");
    }
    let hash = file_sha256(&candidate)?;
    let directory = reserve_revision(&path, "render")?;
    let evidence = serde_json::json!({
        "format_version": FORMAT_VERSION, "output": output, "sha256": hash,
        "captions": selected, "review": project.acceptance,
        "renderer": renderer.renderer, "renderer_reasons": renderer.reasons,
        "font": style.font_label(), "size": style.size,
        "verification": verification,
        "visual_review": "Inspect the rendered video for caption placement, clipping and watermark appearance. Decode and stream checks do not establish visual or linguistic correctness."
    });
    atomic_write(
        &path.join(&directory).join("verification.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    // Hard-linking a completed sibling file publishes atomically and never clobbers
    // another writer's output. Both paths are on the destination filesystem.
    fs::hard_link(&candidate, &output)
        .with_context(|| format!("could not publish new output {}", output.display()))?;
    project.renders.push(RenderRecord {
        output,
        sha256: hash,
        verification: format!("{directory}/verification.json"),
        fingerprint: expected,
    });
    save_project(&path, &project)?;
    let status = status_for(&path, &project)?;
    write_review(&path, &project, &status)?;
    Ok(status)
}

fn status_for(path: &Path, project: &Project) -> Result<WorkflowStatus> {
    let config = project_config(path)?;
    verify_raw(path, project)?;
    let mut status = WorkflowStatus {
        fingerprint: None,
        project: path.to_path_buf(),
        state: "transcription_pending".to_owned(),
        media: project.media.clone(),
        raw_subtitles: project.raw_sha256.as_ref().map(|_| path.join(RAW)),
        source_subtitles: None,
        selected_subtitles: None,
        review_file: path.join("review.md"),
        issues: Vec::new(),
        manually_edited_source: false,
        manually_edited_selected: false,
        next_action: "Run `lilaccaps workflow resume <project>`.".to_owned(),
        last_output: project.renders.last().map(|item| item.output.clone()),
    };
    let Some(source) = &project.source else {
        if project.raw_sha256.is_some() {
            status.state = "source_review_pending".to_owned();
        }
        return Ok(status);
    };
    let source_path = captions_path(path, source, "source")?;
    let source_hash = file_sha256(&source_path)?;
    let source_cues = parse_srt_file(&source_path)?;
    status.issues.extend(stage_issues(path, source, "source")?);
    status.issues.extend(check_cues(
        &source_cues,
        project.duration_seconds,
        &config.transcribe.cues,
        "source",
    )?);
    status.manually_edited_source = source.generated_sha256 != source_hash;
    status.source_subtitles = Some(source_path.clone());
    if project.target_language.is_some() {
        let Some(target) = &project.target else {
            status.state = "translation_pending".to_owned();
            return Ok(status);
        };
        if target.input_sha256 != source_hash {
            status.state = "translation_stale".to_owned();
            status.next_action = "Source captions changed. Run `lilaccaps workflow resume <project>` to create a new translation revision.".to_owned();
            return Ok(status);
        }
        let target_path = captions_path(path, target, "target")?;
        let target_cues = parse_srt_file(&target_path)?;
        ensure_alignment(&source_cues, &target_cues)?;
        status.issues.extend(stage_issues(path, target, "target")?);
        status.issues.extend(check_cues(
            &target_cues,
            project.duration_seconds,
            &config.transcribe.cues,
            "target",
        )?);
        status.manually_edited_selected = target.generated_sha256 != file_sha256(&target_path)?;
        status.selected_subtitles = Some(target_path);
    } else {
        status.manually_edited_selected = status.manually_edited_source;
        status.selected_subtitles = Some(source_path);
    }
    status.state = "review_required".to_owned();
    status.next_action = "Review the SRT and review.md, resolve or acknowledge the issues, then run `lilaccaps workflow accept <project> --note <review-note>`.".to_owned();
    let current = fingerprint(path, project)?;
    status.fingerprint = Some(current.clone());
    if project
        .acceptance
        .as_ref()
        .is_some_and(|review| review.fingerprint == current)
    {
        status.state = "accepted".to_owned();
        status.next_action =
            "Run `lilaccaps workflow render <project>`; font and size can be set for this render."
                .to_owned();
        if project.renders.last().is_some_and(|item| {
            item.fingerprint == current
                && file_sha256(&item.output).is_ok_and(|hash| hash == item.sha256)
        }) {
            status.state = "rendered".to_owned();
            status.next_action =
                "Inspect the rendered video and its saved verification report.".to_owned();
        }
    }
    Ok(status)
}

fn check_cues(
    cues: &[SrtCue],
    duration: f64,
    policy: &TranscribeCueConfig,
    stage: &str,
) -> Result<Vec<WorkflowIssue>> {
    if cues.is_empty() {
        bail!("{stage} captions contain no cues");
    }
    let mut indexes = HashSet::new();
    let mut previous_end = 0;
    let mut issues = Vec::new();
    for cue in cues {
        if cue.index == 0 || !indexes.insert(cue.index) {
            bail!(
                "{stage} captions have an invalid or duplicate cue ID {}",
                cue.index
            );
        }
        if cue.text.trim().is_empty() || cue.start_ms < previous_end || cue.end_ms <= cue.start_ms {
            bail!(
                "{stage} cue {} has empty text, invalid timing or overlaps the previous cue",
                cue.index
            );
        }
        if cue.end_ms as f64 > duration * 1_000.0 + 50.0 {
            bail!("{stage} cue {} extends beyond the video", cue.index);
        }
        previous_end = cue.end_ms;
        let mut warn = |kind: &str, message: String| {
            issues.push(WorkflowIssue {
                stage: stage.to_owned(),
                cue_id: cue.index,
                kind: kind.to_owned(),
                message,
            })
        };
        let length = (cue.end_ms - cue.start_ms) as u64;
        if length < policy.min_duration_ms {
            warn(
                "short_cue",
                format!("Cue lasts {length}ms; check whether it can be read comfortably."),
            );
        }
        if length > policy.max_duration_ms {
            warn("long_cue", format!("Cue lasts {length}ms."));
        }
        if cue.text.lines().count() > policy.max_lines {
            warn(
                "line_count",
                "Too many caption lines; check placement and wrapping.".to_owned(),
            );
        }
        for line in cue.text.lines() {
            let cjk = line.chars().any(|c| matches!(c, '\u{3400}'..='\u{9fff}' | '\u{3040}'..='\u{30ff}' | '\u{ac00}'..='\u{d7af}'));
            let limit = if cjk {
                policy.max_cjk_chars_per_line
            } else {
                policy.max_chars_per_line
            };
            if line.chars().count() > limit {
                warn(
                    "line_length",
                    format!(
                        "Line exceeds {limit} characters; inspect wrapping at the chosen font size."
                    ),
                );
            }
        }
    }
    Ok(issues)
}

fn ensure_alignment(source: &[SrtCue], target: &[SrtCue]) -> Result<()> {
    if source.len() != target.len()
        || source
            .iter()
            .zip(target)
            .any(|(a, b)| a.index != b.index || a.start_ms != b.start_ms || a.end_ms != b.end_ms)
    {
        bail!(
            "translation cue IDs or timestamps differ from source captions; restore alignment before acceptance"
        );
    }
    Ok(())
}

fn save_stage(
    path: &Path,
    kind: &str,
    input_hash: &str,
    reviewed: ReviewedCaptions,
) -> Result<Stage> {
    let directory = reserve_revision(path, kind)?;
    let dir = path.join(&directory);
    write_srt_file(&dir.join("captions.srt"), &reviewed.cues)?;
    atomic_write(
        &dir.join("review.json"),
        serde_json::to_vec_pretty(&reviewed.report)?,
    )?;
    Ok(Stage {
        directory,
        input_sha256: input_hash.to_owned(),
        generated_sha256: file_sha256(&dir.join("captions.srt"))?,
        report_sha256: file_sha256(&dir.join("review.json"))?,
    })
}

fn reserve_revision(path: &Path, kind: &str) -> Result<String> {
    for index in 1..1_000_000 {
        let name = format!("{kind}-{index:04}");
        match fs::create_dir(path.join(&name)) {
            Ok(()) => return Ok(name),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    bail!("no available revision directory")
}

fn stage_directory(path: &Path, stage: &Stage, kind: &str) -> Result<PathBuf> {
    let Some(number) = stage.directory.strip_prefix(&format!("{kind}-")) else {
        bail!("invalid {kind} revision directory");
    };
    if number.is_empty() || !number.bytes().all(|c| c.is_ascii_digit()) {
        bail!("invalid {kind} revision directory");
    }
    let dir = path.join(&stage.directory);
    if !fs::symlink_metadata(&dir)?.file_type().is_dir() {
        bail!(
            "revision directory must not be a symlink: {}",
            dir.display()
        );
    }
    Ok(dir)
}

fn captions_path(path: &Path, stage: &Stage, kind: &str) -> Result<PathBuf> {
    let result = stage_directory(path, stage, kind)?.join("captions.srt");
    regular_file(&result)?;
    Ok(result)
}

fn stage_issues(path: &Path, stage: &Stage, kind: &str) -> Result<Vec<WorkflowIssue>> {
    let report_path = stage_directory(path, stage, kind)?.join("review.json");
    regular_file(&report_path)?;
    if file_sha256(&report_path)? != stage.report_sha256 {
        bail!("saved {kind} agent report changed; edit captions instead of the review evidence");
    }
    let report: ReviewReport =
        serde_json::from_str(&read_text_bounded(&report_path, 16 * 1024 * 1024)?)?;
    Ok(report
        .issues
        .into_iter()
        .map(|issue| WorkflowIssue {
            stage: kind.to_owned(),
            cue_id: issue.cue_id,
            kind: issue.kind,
            message: issue.message,
        })
        .collect())
}

fn fingerprint(path: &Path, project: &Project) -> Result<ReviewFingerprint> {
    let source = project
        .source
        .as_ref()
        .context("source review is not ready")?;
    stage_issues(path, source, "source")?;
    let source_hash = file_sha256(&captions_path(path, source, "source")?)?;
    let (selected_sha256, target_report_sha256) = if project.target_language.is_some() {
        let target = project
            .target
            .as_ref()
            .context("translation is not ready")?;
        stage_issues(path, target, "target")?;
        if target.input_sha256 != source_hash {
            bail!("translation needs refreshing after source edits");
        }
        (
            file_sha256(&captions_path(path, target, "target")?)?,
            Some(target.report_sha256.clone()),
        )
    } else {
        (source_hash.clone(), None)
    };
    Ok(ReviewFingerprint {
        source_sha256: source_hash,
        selected_sha256,
        source_report_sha256: source.report_sha256.clone(),
        target_report_sha256,
        watermark_sha256: if project.watermark.is_some() {
            watermark_presets::load_snapshot(&path.join("watermark"))?;
            Some(directory_sha256(&path.join("watermark"))?)
        } else {
            None
        },
    })
}

fn project_path(path: &Path) -> Result<PathBuf> {
    let result = fs::canonicalize(path)
        .with_context(|| format!("cannot open project {}", path.display()))?;
    if !result.is_dir() {
        bail!("project must be a directory");
    }
    Ok(result)
}

fn regular_file(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)
        .with_context(|| format!("missing project file {}", path.display()))?
        .file_type()
        .is_file()
    {
        bail!(
            "project file must be a regular file, not a symlink: {}",
            path.display()
        );
    }
    Ok(())
}

fn lock_project(path: &Path) -> Result<File> {
    let lock = path.join(".lock");
    if fs::symlink_metadata(&lock).is_ok() {
        regular_file(&lock)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)?;
    file.try_lock()
        .context("another command is using this caption project")?;
    Ok(file)
}

fn load_project(path: &Path) -> Result<Project> {
    let file = path.join(MANIFEST);
    regular_file(&file)?;
    let project: Project = serde_json::from_str(&read_text_bounded(&file, 16 * 1024 * 1024)?)?;
    if project.format_version != FORMAT_VERSION {
        bail!(
            "unsupported caption project version {}",
            project.format_version
        );
    }
    if !project.media.is_absolute()
        || !project.duration_seconds.is_finite()
        || project.duration_seconds <= 0.0
    {
        bail!("invalid project media metadata");
    }
    Ok(project)
}

fn save_project(path: &Path, project: &Project) -> Result<()> {
    atomic_write(&path.join(MANIFEST), serde_json::to_vec_pretty(project)?)
}

fn project_config(path: &Path) -> Result<Config> {
    regular_file(&path.join(CONFIG))?;
    Ok(load_config(Some(path.join(CONFIG)))?.config)
}

fn verify_media(project: &Project) -> Result<()> {
    if file_sha256(&project.media)? != project.media_sha256 {
        bail!("source video changed; start a new caption project for the changed media");
    }
    Ok(())
}

fn verify_raw(path: &Path, project: &Project) -> Result<()> {
    if let Some(expected) = &project.raw_sha256 {
        regular_file(&path.join(RAW))?;
        if &file_sha256(&path.join(RAW))? != expected {
            bail!(
                "raw.srt changed; keep this checkpoint intact and edit the source revision instead"
            );
        }
    }
    Ok(())
}

fn read_text_bounded(path: &Path, limit: u64) -> Result<String> {
    let mut value = String::new();
    File::open(path)?
        .take(limit + 1)
        .read_to_string(&mut value)?;
    if value.len() as u64 > limit {
        bail!("text file is too large: {}", path.display());
    }
    Ok(value)
}

pub(crate) fn file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn directory_sha256(path: &Path) -> Result<String> {
    let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut hash = Sha256::new();
    for entry in entries {
        let metadata = entry.file_type()?;
        hash.update(entry.file_name().to_string_lossy().as_bytes());
        hash.update([0]);
        if metadata.is_file() {
            if entry.file_name() == "preset.json" {
                // Snapshotting normalises JSON formatting. Bind the definition's
                // values and every asset's bytes, rather than JSON key order.
                let definition: serde_json::Value =
                    serde_json::from_str(&read_text_bounded(&entry.path(), 64 * 1024)?)?;
                hash.update(serde_json::to_vec(&definition)?);
            } else {
                hash.update(file_sha256(&entry.path())?.as_bytes());
            }
        } else if metadata.is_dir() {
            hash.update(directory_sha256(&entry.path())?.as_bytes());
        } else {
            bail!("watermark snapshot contains a symlink or unsupported file");
        }
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    // Preserve the accepted definition verbatim: loading a preset may normalise
    // strings, so reserialising it here would change its fingerprint.
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            bail!("watermark snapshot contains a symlink or unsupported file");
        }
    }
    Ok(())
}

fn write_review(path: &Path, project: &Project, status: &WorkflowStatus) -> Result<()> {
    let mut text = format!(
        "# Caption review\n\nState: **{}**\n\nSource media: `{}`\n\n",
        status.state,
        project.media.display()
    );
    for (label, file) in [
        ("Raw transcript", &status.raw_subtitles),
        ("Reviewed source", &status.source_subtitles),
        ("Selected captions", &status.selected_subtitles),
    ] {
        if let Some(file) = file {
            text.push_str(&format!(
                "- {label}: [{}]({})\n",
                file.file_name().unwrap_or_default().to_string_lossy(),
                file.display()
            ));
        }
    }
    text.push_str("\nThe agent used text and neighbouring cues. It did not listen to the audio. Check uncertain phrases against the recording, then review names, numbers, meaning, and reading speed. Each revision has a JSON report. Edit the selected SRT to make corrections. Source edits after translation require `workflow resume` to create a fresh target revision; previous files are kept.\n");
    text.push_str(&format!("\nSource edited after its agent pass: **{}**. Selected captions edited after their agent pass: **{}**.\n", status.manually_edited_source, status.manually_edited_selected));
    if let Some(name) = &project.watermark {
        text.push_str(&format!(
            "\nWatermark: `{name}` (a project copy with its own assets).\n"
        ));
    }
    text.push_str("\n## Review issues\n\n");
    if status.issues.is_empty() {
        text.push_str("No automated issues were found. This does not establish transcription or translation accuracy.\n");
    }
    for issue in &status.issues {
        text.push_str(&format!(
            "- {} cue {} / {}: {}\n",
            issue.stage,
            issue.cue_id,
            issue.kind,
            issue.message.replace('\n', " ")
        ));
    }
    if let Some(review) = &project.acceptance {
        text.push_str(&format!("\n## Last acceptance\n\n{}\n\nAcknowledged issues: {}. The acceptance applies only while the captions and watermark match the recorded fingerprints.\n", review.note, review.issues_acknowledged));
    }
    text.push_str(&format!("\n## Next action\n\n{}\n", status.next_action));
    if let Some(record) = project.renders.last() {
        text.push_str(&format!(
            "\nOutput: [{}]({})\n\nVerification: [{}]({})\n",
            record.output.display(),
            record.output.display(),
            record.verification,
            path.join(&record.verification).display()
        ));
    }
    atomic_write(&path.join("review.md"), text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millisecond_alignment_is_not_rounded() {
        let source = vec![SrtCue {
            index: 7,
            start_ms: 123,
            end_ms: 1987,
            text: "Hello".into(),
        }];
        let mut target = source.clone();
        target[0].text = "你好".into();
        assert!(ensure_alignment(&source, &target).is_ok());
        target[0].start_ms += 1;
        assert!(ensure_alignment(&source, &target).is_err());
    }

    #[test]
    fn structural_checks_reject_empty_duplicate_overlap_and_out_of_bounds_cues() {
        let policy = TranscribeCueConfig::default();
        let cue = SrtCue {
            index: 7,
            start_ms: 123,
            end_ms: 1987,
            text: "Hello".into(),
        };
        assert!(check_cues(&[], 2.0, &policy, "source").is_err());
        assert!(check_cues(&[cue.clone(), cue.clone()], 3.0, &policy, "source").is_err());
        assert!(check_cues(std::slice::from_ref(&cue), 1.0, &policy, "source").is_err());
        assert!(check_cues(&[cue], 2.0, &policy, "source").is_ok());
    }

    #[test]
    fn revision_paths_cannot_escape_project() {
        let stage = Stage {
            directory: "source-../../secret".into(),
            input_sha256: String::new(),
            generated_sha256: String::new(),
            report_sha256: String::new(),
        };
        assert!(stage_directory(Path::new("/tmp"), &stage, "source").is_err());
    }
}
