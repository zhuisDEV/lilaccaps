use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::TranscribeCleanupConfig;
use crate::runtime::{ScopedTempPath, ensure_dir, tmp_dir};
use crate::subtitles::SubtitleCue;

const OUTPUT_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "required": ["cues"],
  "properties": {
    "cues": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["index", "text"],
        "properties": {
          "index": {"type": "integer", "minimum": 1},
          "text": {"type": "string", "minLength": 1}
        }
      }
    }
  }
}"#;

#[derive(Debug, Serialize)]
struct CleanupInputCue<'a> {
    index: usize,
    text: &'a str,
}

#[derive(Debug, Deserialize)]
struct CleanupOutput {
    cues: Vec<CleanupOutputCue>,
}

#[derive(Debug, Deserialize)]
struct CleanupOutputCue {
    index: usize,
    text: String,
}

pub fn clean_cues(
    runtime_home: &Path,
    language: &str,
    config: &TranscribeCleanupConfig,
    cues: &[SubtitleCue],
) -> Result<Vec<SubtitleCue>> {
    if cues.is_empty() {
        bail!("cannot clean an empty subtitle transcript");
    }
    let temp_root = tmp_dir(runtime_home);
    ensure_dir(&temp_root)?;
    let workdir = ScopedTempPath::directory(&temp_root, "cleanup-work")?;
    let schema_path = ScopedTempPath::file(workdir.path(), "cleanup-schema", Some("json"));
    let output_path = ScopedTempPath::file(workdir.path(), "cleanup-output", Some("json"));
    fs::write(schema_path.path(), OUTPUT_SCHEMA).with_context(|| {
        format!(
            "failed to write cleanup schema {}",
            schema_path.path().display()
        )
    })?;

    let prompt = cleanup_prompt(language, cues)?;
    let model = config.model.strip_prefix("codex/").unwrap_or(&config.model);
    let mut child = Command::new(&config.command)
        .arg("exec")
        .arg("--sandbox")
        .arg("read-only")
        .arg("--ephemeral")
        .arg("--skip-git-repo-check")
        .arg("--output-schema")
        .arg(schema_path.path())
        .arg("--output-last-message")
        .arg(output_path.path())
        .arg("--model")
        .arg(model)
        .arg("--config")
        .arg(format!(
            "model_reasoning_effort={}",
            toml::Value::String(config.reasoning_effort.clone())
        ))
        .arg("-")
        .current_dir(workdir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start cleanup command `{}`", config.command))?;
    child
        .stdin
        .as_mut()
        .context("cleanup command stdin was unavailable")?
        .write_all(prompt.as_bytes())
        .context("failed to send subtitle cleanup prompt")?;
    let output = child
        .wait_with_output()
        .context("failed while waiting for subtitle cleanup")?;
    if !output.status.success() {
        bail!(
            "subtitle cleanup failed with {}: {}",
            output.status,
            concise_failure(&output.stderr)
        );
    }

    let raw = fs::read_to_string(output_path.path()).with_context(|| {
        format!(
            "cleanup command did not produce {}",
            output_path.path().display()
        )
    })?;
    let cleaned: CleanupOutput =
        serde_json::from_str(&raw).context("cleanup output was not valid structured JSON")?;
    validate_and_apply_cleanup(cues, cleaned)
}

fn concise_failure(stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let message = stderr
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with("ERROR:"))
        .or_else(|| stderr.lines().rev().find(|line| !line.trim().is_empty()))
        .unwrap_or("cleanup command failed without an error message")
        .trim();
    message.chars().take(500).collect()
}

fn cleanup_prompt(language: &str, cues: &[SubtitleCue]) -> Result<String> {
    let input = cues
        .iter()
        .map(|cue| CleanupInputCue {
            index: cue.index,
            text: &cue.text,
        })
        .collect::<Vec<_>>();
    let json = serde_json::to_string(&input).context("failed to encode cleanup input")?;
    let language_guidance = if matches!(language, "zh" | "zh-cn" | "zh-tw" | "yue") {
        " Use Chinese punctuation. Correct only clear character, homophone, or word-boundary errors; preserve uncertain wording exactly."
    } else {
        ""
    };
    Ok(format!(
        "You are conservatively cleaning {language} subtitle text after speech recognition. Fix punctuation, casing, spacing, and only obvious recognition errors.{language_guidance} Do not translate, summarize, add facts, invent unclear speech, merge cues, split cues, reorder cues, or add line breaks. Return every input index exactly once using the required JSON schema. Input cues: {json}"
    ))
}

fn validate_and_apply_cleanup(
    original: &[SubtitleCue],
    cleaned: CleanupOutput,
) -> Result<Vec<SubtitleCue>> {
    if cleaned.cues.len() != original.len() {
        bail!(
            "cleanup changed cue count from {} to {}",
            original.len(),
            cleaned.cues.len()
        );
    }

    original
        .iter()
        .zip(cleaned.cues)
        .map(|(source, cleaned)| {
            if cleaned.index != source.index {
                bail!(
                    "cleanup changed cue order: expected index {}, received {}",
                    source.index,
                    cleaned.index
                );
            }
            let text = cleaned.text.trim().to_string();
            if text.is_empty() {
                bail!("cleanup returned empty text for cue {}", source.index);
            }
            if text.contains(['\n', '\r']) {
                bail!("cleanup added a line break to cue {}", source.index);
            }
            if is_wholesale_rewrite(&source.text, &text) {
                bail!(
                    "cleanup rewrote too much text in cue {}; refusing a potentially invented or translated result",
                    source.index
                );
            }
            Ok(SubtitleCue {
                index: source.index,
                start_cs: source.start_cs,
                end_cs: source.end_cs,
                text,
            })
        })
        .collect()
}

fn is_wholesale_rewrite(source: &str, cleaned: &str) -> bool {
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
    use super::{
        CleanupOutput, CleanupOutputCue, clean_cues, cleanup_prompt, concise_failure,
        is_wholesale_rewrite, validate_and_apply_cleanup,
    };
    use crate::config::TranscribeCleanupConfig;
    use crate::runtime::ScopedTempPath;
    use crate::subtitles::SubtitleCue;

    fn source_cues() -> Vec<SubtitleCue> {
        vec![SubtitleCue {
            index: 1,
            start_cs: 100,
            end_cs: 200,
            text: "hello world".to_string(),
        }]
    }

    #[test]
    fn prompt_contains_conservative_chinese_constraints() {
        let prompt = cleanup_prompt("zh", &source_cues()).expect("prompt should build");
        assert!(prompt.contains("Do not translate"));
        assert!(prompt.contains("invent unclear speech"));
        assert!(prompt.contains("Use Chinese punctuation"));
        assert!(prompt.contains("preserve uncertain wording exactly"));
        assert!(prompt.contains("hello world"));
    }

    #[test]
    fn cleanup_can_only_replace_text() {
        let source = source_cues();
        let cleaned = validate_and_apply_cleanup(
            &source,
            CleanupOutput {
                cues: vec![CleanupOutputCue {
                    index: 1,
                    text: "Hello, world.".to_string(),
                }],
            },
        )
        .expect("valid cleanup should apply");
        assert_eq!(cleaned[0].text, "Hello, world.");
        assert_eq!(cleaned[0].start_cs, source[0].start_cs);
        assert_eq!(cleaned[0].end_cs, source[0].end_cs);
    }

    #[test]
    fn cleanup_rejects_count_order_and_line_break_changes() {
        let source = source_cues();
        assert!(validate_and_apply_cleanup(&source, CleanupOutput { cues: Vec::new() }).is_err());
        assert!(
            validate_and_apply_cleanup(
                &source,
                CleanupOutput {
                    cues: vec![CleanupOutputCue {
                        index: 2,
                        text: "hello".to_string(),
                    }],
                },
            )
            .is_err()
        );
        assert!(
            validate_and_apply_cleanup(
                &source,
                CleanupOutput {
                    cues: vec![CleanupOutputCue {
                        index: 1,
                        text: "hello\nworld".to_string(),
                    }],
                },
            )
            .is_err()
        );
    }

    #[test]
    fn cleanup_rejects_wholesale_rewrites_but_allows_conservative_edits() {
        assert!(!is_wholesale_rewrite("hello world", "Hello, world."));
        assert!(!is_wholesale_rewrite("今天天气很好", "今天天气真好。"));
        assert!(is_wholesale_rewrite("hello world", "完全不同内容"));
    }

    #[test]
    fn command_failure_hides_the_prompt_and_prefers_the_final_error() {
        let stderr = b"user\nprivate subtitle text\nERROR: account unavailable\n";
        let failure = concise_failure(stderr);
        assert_eq!(failure, "ERROR: account unavailable");
        assert!(!failure.contains("private subtitle"));
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_subprocess_preserves_structure_end_to_end() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let runtime = ScopedTempPath::directory(&std::env::temp_dir(), "lilaccaps-cleanup-test")
            .expect("runtime directory should be created");
        let command = runtime.path().join("mock-codex");
        fs::write(
            &command,
            r#"#!/bin/sh
effort=''
for arg in "$@"; do
  case "$arg" in model_reasoning_effort=*) effort="$arg" ;; esac
done
expected=$(cat ../expected-effort)
[ "$effort" = "model_reasoning_effort=\"$expected\"" ] || exit 9
output=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = '--output-last-message' ]; then
    shift
    output="$1"
  fi
  shift
done
printf '%s' '{"cues":[{"index":1,"text":"Hello, world."}]}' > "$output"
"#,
        )
        .expect("mock cleanup command should be written");
        let mut permissions = fs::metadata(&command)
            .expect("mock cleanup command should exist")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&command, permissions)
            .expect("mock cleanup command should be executable");
        let config = TranscribeCleanupConfig {
            enabled: true,
            command: command.to_string_lossy().into_owned(),
            model: "test-model".to_string(),
            ..TranscribeCleanupConfig::default()
        };

        let mut config = config;
        fs::create_dir_all(runtime.path().join("tmp")).unwrap();
        for effort in ["medium", "high"] {
            config.reasoning_effort = effort.to_string();
            fs::write(runtime.path().join("tmp/expected-effort"), effort).unwrap();
            let cleaned = clean_cues(runtime.path(), "en", &config, &source_cues())
                .expect("mock cleanup should succeed");
            assert_eq!(cleaned[0].text, "Hello, world.");
            assert_eq!(cleaned[0].start_cs, 100);
            assert_eq!(cleaned[0].end_cs, 200);
        }
    }
}
