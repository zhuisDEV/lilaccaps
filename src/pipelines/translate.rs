use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::caption_agent::{ReviewReport, translate_cues};
use crate::config::load_config;
use crate::runtime::{
    ScopedTempPath, atomic_write, ensure_parent_dir, parent_dir, paths_refer_to_same_file,
};
use crate::subtitles::{parse_srt_file, write_srt_file};

#[derive(Debug, Clone)]
pub struct TranslateOutput {
    pub input: PathBuf,
    pub output: PathBuf,
    pub targets: Vec<String>,
    pub append: bool,
    pub model: String,
    pub verification_model: String,
    pub review_report: PathBuf,
    pub review_issues: usize,
    pub status: &'static str,
}

#[derive(Serialize)]
struct TranslationReview<'a> {
    schema_version: u32,
    input: &'a Path,
    output: &'a Path,
    targets: Vec<TargetReview>,
}

#[derive(Serialize)]
struct TargetReview {
    language: String,
    review: ReviewReport,
}

pub fn run(
    input: PathBuf,
    config_path: Option<PathBuf>,
    output: Option<PathBuf>,
    targets: Vec<String>,
    append: Option<bool>,
) -> Result<TranslateOutput> {
    if !input.exists() {
        bail!("subtitle input does not exist: {}", input.display());
    }

    let loaded = load_config(config_path)?;
    let targets = resolve_targets(&loaded.config.translate.default_targets, targets)?;
    let append = append.unwrap_or(loaded.config.translate.append);
    let model = loaded.config.translate.model.clone();
    let verification_model = loaded.config.translate.review_model.clone();
    let line_order = loaded.config.translate.line_order.clone();
    let output = output.unwrap_or_else(|| default_output_path(&input, append));
    if paths_refer_to_same_file(&input, &output)? {
        bail!(
            "translation output must be different from subtitle input: {}",
            input.display()
        );
    }
    let review_report = output.with_file_name(format!(
        "{}.review.json",
        output
            .file_name()
            .context("translation output needs a filename")?
            .to_string_lossy()
    ));
    if paths_refer_to_same_file(&input, &review_report)?
        || paths_refer_to_same_file(&output, &review_report)?
    {
        bail!("translation review report must be a separate file from the input and output");
    }
    for destination in [&output, &review_report] {
        if destination.exists() && !destination.is_file() {
            bail!(
                "translation destination is not a regular file: {}",
                destination.display()
            );
        }
    }
    ensure_parent_dir(&output).with_context(|| {
        format!(
            "failed to create output directory for translation {}",
            output.display()
        )
    })?;

    let mut cues = parse_srt_file(&input)?;
    if cues.is_empty() {
        bail!("subtitle input contained no cues: {}", input.display());
    }

    let source_lines = cues.iter().map(|cue| cue.text.clone()).collect::<Vec<_>>();
    let translated_per_target = targets
        .iter()
        .map(|target| {
            translate_cues(
                &loaded.paths.runtime_home,
                &loaded.config.translate,
                target,
                &cues,
                "",
            )
        })
        .collect::<Result<Vec<_>>>()?;

    for (index, cue) in cues.iter_mut().enumerate() {
        let mut labeled_lines = Vec::new();
        if append {
            labeled_lines.push(("source".to_string(), source_lines[index].clone()));
        }
        for (target_index, translated) in translated_per_target.iter().enumerate() {
            labeled_lines.push((
                targets[target_index].clone(),
                translated.cues[index].text.clone(),
            ));
        }
        cue.text = reorder_labeled_lines(&line_order, labeled_lines).join("\n");
    }

    let review_issues = translated_per_target
        .iter()
        .map(|result| result.report.issues.len())
        .sum();
    let report = TranslationReview {
        schema_version: 1,
        input: &input,
        output: &output,
        targets: targets
            .iter()
            .cloned()
            .zip(translated_per_target)
            .map(|(language, result)| TargetReview {
                language,
                review: result.report,
            })
            .collect(),
    };
    // Finish every target and stage both artefacts before replacing an output.
    // Malformed responses or a later target failure never publish partial captions.
    let staged_srt = ScopedTempPath::file(parent_dir(&output), "translated", Some("srt"));
    let staged_report =
        ScopedTempPath::file(parent_dir(&output), "translation-review", Some("json"));
    write_srt_file(staged_srt.path(), &cues)?;
    atomic_write(staged_report.path(), serde_json::to_vec_pretty(&report)?)?;
    let previous_report = if review_report.exists() {
        Some(std::fs::read(&review_report).context("failed to preserve existing review report")?)
    } else {
        None
    };
    staged_report.persist(&review_report)?;
    if let Err(error) = staged_srt.persist(&output) {
        match previous_report {
            Some(contents) => atomic_write(&review_report, contents).context(
                "subtitle publication failed and the previous report could not be restored",
            )?,
            None => std::fs::remove_file(&review_report).context(
                "subtitle publication failed and the staged review report could not be removed",
            )?,
        }
        return Err(error);
    }

    Ok(TranslateOutput {
        input,
        output,
        targets,
        append,
        model,
        verification_model,
        review_report,
        review_issues,
        status: "translated",
    })
}

fn resolve_targets(config_targets: &[String], cli_targets: Vec<String>) -> Result<Vec<String>> {
    let targets = if cli_targets.is_empty() {
        config_targets.to_vec()
    } else {
        cli_targets
    };
    let targets = targets
        .into_iter()
        .map(|target| target.trim().to_string())
        .filter(|target| !target.is_empty())
        .collect::<Vec<_>>();

    if targets.is_empty() {
        bail!(
            "no translation targets configured; pass one or more --to <lang> flags or set translate.default_targets in lilaccaps.toml"
        );
    }

    Ok(targets)
}

fn default_output_path(input: &Path, append: bool) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|item| item.to_str())
        .unwrap_or("subtitles");
    let suffix = if append { "multilang" } else { "translated" };
    input.with_file_name(format!("{stem}.{suffix}.srt"))
}

fn reorder_labeled_lines(
    line_order: &[String],
    labeled_lines: Vec<(String, String)>,
) -> Vec<String> {
    if line_order.is_empty() {
        return labeled_lines.into_iter().map(|(_, line)| line).collect();
    }

    let mut ordered = Vec::new();
    let mut remaining = labeled_lines;

    for role in line_order {
        if let Some(index) = remaining.iter().position(|(label, _)| label == role) {
            ordered.push(remaining.remove(index).1);
        }
    }

    ordered.extend(remaining.into_iter().map(|(_, line)| line));
    ordered
}

#[cfg(test)]
mod tests {
    use super::{default_output_path, reorder_labeled_lines, resolve_targets};
    use std::path::Path;

    #[test]
    fn cli_targets_override_config_targets() {
        let targets = resolve_targets(&["ja".to_string()], vec!["en".to_string()])
            .expect("targets should resolve");
        assert_eq!(targets, vec!["en"]);
    }

    #[test]
    fn append_output_path_uses_multilang_suffix() {
        let path = default_output_path(Path::new("/tmp/input.srt"), true);
        assert_eq!(path, Path::new("/tmp/input.multilang.srt"));
    }

    #[test]
    fn line_order_reorders_source_and_targets() {
        let lines = reorder_labeled_lines(
            &["ja".to_string(), "source".to_string(), "en".to_string()],
            vec![
                ("source".to_string(), "原文".to_string()),
                ("en".to_string(), "English".to_string()),
                ("ja".to_string(), "日本語".to_string()),
            ],
        );
        assert_eq!(lines, vec!["日本語", "原文", "English"]);
    }
}
