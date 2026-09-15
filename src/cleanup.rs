use std::path::Path;

use anyhow::{Context, Result};

use crate::caption_agent::review_transcript;
use crate::config::TranscribeCleanupConfig;
use crate::subtitles::{SrtCue, SubtitleCue};

/// The legacy cleanup entry point now shares the same bounded agent generation
/// and independent verification as the review workflow.
pub fn clean_cues(
    runtime_home: &Path,
    language: &str,
    config: &TranscribeCleanupConfig,
    cues: &[SubtitleCue],
) -> Result<Vec<SubtitleCue>> {
    let source = cues
        .iter()
        .map(|cue| {
            Ok(SrtCue {
                index: cue.index,
                start_ms: cue
                    .start_cs
                    .checked_mul(10)
                    .context("cue start timestamp is too large")?,
                end_ms: cue
                    .end_cs
                    .checked_mul(10)
                    .context("cue end timestamp is too large")?,
                text: cue.text.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let reviewed = review_transcript(
        runtime_home,
        config,
        &source,
        &format!("Transcript language: {language}"),
    )?;
    for issue in &reviewed.report.issues {
        eprintln!(
            "cleanup_review_issue = cue {}: {}: {}",
            issue.cue_id, issue.kind, issue.message
        );
    }
    Ok(cues
        .iter()
        .zip(reviewed.cues)
        .map(|(source, reviewed)| SubtitleCue {
            text: reviewed.text,
            ..source.clone()
        })
        .collect())
}

pub(crate) fn is_wholesale_rewrite(source: &str, cleaned: &str) -> bool {
    let source = comparison_chars(source);
    let cleaned = comparison_chars(cleaned);
    let longest = source.len().max(cleaned.len());
    longest >= 5 && edit_distance(&source, &cleaned) * 2 > longest
}

fn comparison_chars(text: &str) -> Vec<char> {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn edit_distance(left: &[char], right: &[char]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_character) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_character) in right.iter().enumerate() {
            let substitution =
                previous[right_index] + usize::from(left_character != right_character);
            current[right_index + 1] = (current[right_index] + 1)
                .min(previous[right_index + 1] + 1)
                .min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::is_wholesale_rewrite;

    #[test]
    fn cleanup_rejects_wholesale_rewrites_but_allows_conservative_edits() {
        assert!(!is_wholesale_rewrite("hello world", "Hello, world."));
        assert!(!is_wholesale_rewrite("今天天气很好", "今天天气真好。"));
        assert!(is_wholesale_rewrite("hello world", "完全不同内容"));
    }
}
