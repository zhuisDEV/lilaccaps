use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::load_config;
use crate::subtitles::{parse_srt_file, write_srt_file};
use crate::translate::translate_lines;

#[derive(Debug, Clone)]
pub struct TranslateOutput {
    pub input: PathBuf,
    pub output: PathBuf,
    pub targets: Vec<String>,
    pub append: bool,
    pub model: String,
    pub status: &'static str,
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
    let line_order = loaded.config.translate.line_order.clone();

    let mut cues = parse_srt_file(&input)?;
    if cues.is_empty() {
        bail!("subtitle input contained no cues: {}", input.display());
    }

    let source_lines = cues.iter().map(|cue| cue.text.clone()).collect::<Vec<_>>();
    let translated_per_target = targets
        .iter()
        .map(|target| translate_lines(&model, target, &source_lines))
        .collect::<Result<Vec<_>>>()?;

    for (index, cue) in cues.iter_mut().enumerate() {
        let mut labeled_lines = Vec::new();
        if append {
            labeled_lines.push(("source".to_string(), source_lines[index].clone()));
        }
        for (target_index, translated) in translated_per_target.iter().enumerate() {
            labeled_lines.push((targets[target_index].clone(), translated[index].clone()));
        }
        cue.text = reorder_labeled_lines(&line_order, labeled_lines).join("\n");
    }

    let output = output.unwrap_or_else(|| default_output_path(&input, append));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create output directory for translation {}",
                output.display()
            )
        })?;
    }
    write_srt_file(&output, &cues)?;

    Ok(TranslateOutput {
        input,
        output,
        targets,
        append,
        model,
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
