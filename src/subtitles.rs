use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::runtime::atomic_write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleCue {
    pub index: usize,
    pub start_cs: i64,
    pub end_cs: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrtCue {
    pub index: usize,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedWord {
    pub start_cs: i64,
    pub end_cs: i64,
    pub text: String,
}

#[derive(Debug, Clone, Copy)]
pub struct CuePolicy {
    pub min_duration_cs: i64,
    pub max_duration_cs: i64,
    pub end_padding_cs: i64,
    pub max_chars_per_line: usize,
    pub max_cjk_chars_per_line: usize,
    pub max_lines: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubtitleQaReport {
    pub warnings: Vec<String>,
}

pub fn build_cues_from_timed_words(
    words: &[TimedWord],
    policy: CuePolicy,
    pause_split_cs: i64,
) -> Vec<SubtitleCue> {
    let words = words
        .iter()
        .filter(|word| word.end_cs > word.start_cs && !word.text.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if words.is_empty() {
        return Vec::new();
    }

    let mut cues = Vec::new();
    let mut start = 0usize;
    while start < words.len() {
        let remaining_text = join_timed_words(&words[start..]);
        let character_limit = cue_character_limit(&remaining_text, policy);
        let remaining_characters = remaining_text.chars().count().max(1);
        let remaining_duration_cs = words
            .last()
            .expect("timed words are non-empty")
            .end_cs
            .saturating_sub(words[start].start_cs)
            .max(1);
        let desired_parts = remaining_characters
            .div_ceil(character_limit)
            .max(
                usize::try_from(ceil_div_positive_i64(
                    remaining_duration_cs,
                    policy.max_duration_cs.max(1),
                ))
                .unwrap_or(usize::MAX),
            )
            .max(1);
        let character_target = remaining_characters.div_ceil(desired_parts).max(1);
        let duration_target_cs = ceil_div_positive_i64(
            remaining_duration_cs,
            i64::try_from(desired_parts).unwrap_or(i64::MAX).max(1),
        )
        .max(policy.min_duration_cs.max(1));
        let mut end = start;
        let mut preferred_end = None;
        let mut rewind_for_punctuation = false;

        while end < words.len() {
            let candidate_text = join_timed_words(&words[start..=end]);
            let duration_cs = words[end].end_cs.saturating_sub(words[start].start_cs);
            let exceeds_limit = candidate_text.chars().count() > character_target;
            let exceeds_duration = duration_cs > duration_target_cs;
            let punctuation_suffix = is_punctuation_only(&words[end].text);
            let exceeds_hard_limit = candidate_text.chars().count() > character_limit;
            let exceeds_hard_duration = duration_cs > policy.max_duration_cs.max(1);
            let next_pause_cs = words
                .get(end + 1)
                .map_or(0, |next| next.start_cs.saturating_sub(words[end].end_cs));
            let natural_boundary = ends_with_strong_punctuation(&candidate_text)
                || ends_with_soft_punctuation(&candidate_text)
                || next_pause_cs >= pause_split_cs.max(1);
            let usable_natural_boundary =
                natural_boundary && !exceeds_hard_limit && !exceeds_hard_duration;
            if end >= start.saturating_add(2) && exceeds_hard_limit && punctuation_suffix {
                rewind_for_punctuation = true;
                break;
            }
            if end > start
                && (exceeds_limit || exceeds_duration)
                && !punctuation_suffix
                && !usable_natural_boundary
            {
                break;
            }

            if natural_boundary {
                preferred_end = Some(end);
                if duration_cs >= policy.min_duration_cs.max(1) {
                    end += 1;
                    break;
                }
            }
            end += 1;
        }

        let hard_end = if rewind_for_punctuation {
            end.saturating_sub(2)
        } else {
            end.saturating_sub(1)
        }
        .max(start)
        .min(words.len() - 1);
        let cue_end = preferred_end
            .filter(|preferred| *preferred <= hard_end)
            .unwrap_or(hard_end);
        let text = join_timed_words(&words[start..=cue_end]);
        if !text.is_empty() {
            cues.push(SubtitleCue {
                index: cues.len() + 1,
                start_cs: words[start].start_cs,
                end_cs: words[cue_end].end_cs,
                text,
            });
        }
        start = cue_end + 1;
    }

    cues
}

fn cue_character_limit(text: &str, policy: CuePolicy) -> usize {
    if text.chars().any(is_cjk_character) {
        policy.max_cjk_chars_per_line
    } else {
        policy.max_chars_per_line
    }
    .max(1)
}

fn ceil_div_positive_i64(numerator: i64, denominator: i64) -> i64 {
    numerator.saturating_add(denominator.max(1).saturating_sub(1)) / denominator.max(1)
}

fn join_timed_words(words: &[TimedWord]) -> String {
    let joined = words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<String>();
    let mut normalized = String::with_capacity(joined.len());
    let mut previous_was_whitespace = false;
    for character in joined.chars() {
        if character.is_whitespace() {
            if !previous_was_whitespace {
                normalized.push(' ');
            }
            previous_was_whitespace = true;
        } else {
            normalized.push(character);
            previous_was_whitespace = false;
        }
    }
    normalized.trim().to_string()
}

fn ends_with_strong_punctuation(text: &str) -> bool {
    text.trim_end()
        .ends_with(['。', '！', '？', '；', '.', '!', '?', ';', '…'])
}

fn ends_with_soft_punctuation(text: &str) -> bool {
    text.trim_end().ends_with(['，', '、', '：', ',', ':'])
}

fn is_punctuation_only(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && text.chars().all(|character| {
            matches!(
                character,
                '。' | '！'
                    | '？'
                    | '；'
                    | '，'
                    | '、'
                    | '：'
                    | '.'
                    | '!'
                    | '?'
                    | ';'
                    | ','
                    | ':'
                    | '…'
            )
        })
}

pub fn optimize_cues(
    cues: Vec<SubtitleCue>,
    media_duration_cs: i64,
    policy: CuePolicy,
) -> Vec<SubtitleCue> {
    let media_duration_cs = media_duration_cs.max(0);
    let min_duration_cs = policy.min_duration_cs.max(1);
    let end_padding_cs = policy.end_padding_cs.max(0);
    let normalized = cues
        .into_iter()
        .filter_map(|mut cue| {
            cue.text = normalize_cue_text(&cue.text);
            if cue.text.is_empty() {
                return None;
            }

            cue.start_cs = cue.start_cs.clamp(0, media_duration_cs);
            if cue.start_cs >= media_duration_cs {
                return None;
            }

            cue.end_cs = cue.end_cs.clamp(0, media_duration_cs);
            if cue.end_cs <= cue.start_cs {
                cue.end_cs = cue
                    .start_cs
                    .saturating_add(min_duration_cs)
                    .min(media_duration_cs);
            }
            (cue.end_cs > cue.start_cs).then_some(cue)
        })
        .collect::<Vec<_>>();
    let mut optimized = normalized
        .into_iter()
        .flat_map(|cue| split_long_cue(cue, policy))
        .map(|mut cue| {
            let minimum_end_cs = cue.start_cs.saturating_add(min_duration_cs);
            cue.end_cs = cue
                .end_cs
                .saturating_add(end_padding_cs)
                .max(minimum_end_cs)
                .min(media_duration_cs);
            cue
        })
        .collect::<Vec<_>>();

    optimized.sort_by_key(|cue| (cue.start_cs, cue.end_cs, cue.index));

    for index in 0..optimized.len().saturating_sub(1) {
        let next_start_cs = optimized[index + 1].start_cs;
        if optimized[index].end_cs > next_start_cs {
            optimized[index].end_cs = next_start_cs;
        }
    }

    optimized.retain(|cue| cue.end_cs > cue.start_cs);
    for (index, cue) in optimized.iter_mut().enumerate() {
        cue.index = index + 1;
    }
    optimized
}

fn split_long_cue(cue: SubtitleCue, policy: CuePolicy) -> Vec<SubtitleCue> {
    let uses_cjk_limit = cue.text.chars().any(is_cjk_character);
    let line_limit = if uses_cjk_limit {
        policy.max_cjk_chars_per_line
    } else {
        policy.max_chars_per_line
    }
    .max(1);
    let cue_capacity = line_limit;
    let character_count = cue.text.chars().count();
    let duration_cs = cue.end_cs.saturating_sub(cue.start_cs).max(1);
    let parts_for_text = character_count.saturating_add(cue_capacity - 1) / cue_capacity;
    let parts_for_duration = usize::try_from(
        duration_cs.saturating_add(policy.max_duration_cs.max(1) - 1)
            / policy.max_duration_cs.max(1),
    )
    .unwrap_or(usize::MAX);
    let duration_parts_with_enough_text = parts_for_duration.min((character_count / 4).max(1));
    let desired_parts = parts_for_text.max(duration_parts_with_enough_text).max(1);
    let maximum_part_characters = character_count
        .saturating_add(desired_parts - 1)
        .checked_div(desired_parts)
        .unwrap_or(1)
        .min(cue_capacity)
        .max(1);
    let parts = split_text_at_boundaries(&cue.text, maximum_part_characters);

    if parts.len() == 1 {
        let mut cue = cue;
        cue.text = parts[0].clone();
        return vec![cue];
    }

    let weights = parts
        .iter()
        .map(|part| part.chars().count().max(1))
        .collect::<Vec<_>>();
    let total_weight = weights.iter().sum::<usize>().max(1);
    let part_count = parts.len();
    let mut cumulative_weight = 0usize;
    let mut split_cues = Vec::with_capacity(parts.len());

    for (index, (part, weight)) in parts.into_iter().zip(weights).enumerate() {
        let start_cs =
            proportional_timestamp(cue.start_cs, duration_cs, cumulative_weight, total_weight);
        cumulative_weight = cumulative_weight.saturating_add(weight);
        let end_cs = if index + 1 == part_count {
            cue.end_cs
        } else {
            proportional_timestamp(cue.start_cs, duration_cs, cumulative_weight, total_weight)
        };
        if end_cs <= start_cs {
            // Keep all text when centisecond timing cannot represent every part.
            // Readability QA can report the unsplit cue instead of losing words.
            return vec![cue];
        }
        split_cues.push(SubtitleCue {
            index: cue.index,
            start_cs,
            end_cs,
            text: part,
        });
    }
    split_cues
}

fn proportional_timestamp(
    start_cs: i64,
    duration_cs: i64,
    completed_weight: usize,
    total_weight: usize,
) -> i64 {
    let offset = (i128::from(duration_cs) * completed_weight as i128 / total_weight.max(1) as i128)
        .min(i128::from(i64::MAX)) as i64;
    start_cs.saturating_add(offset)
}

fn split_text_at_boundaries(text: &str, maximum_characters: usize) -> Vec<String> {
    let characters = text.chars().collect::<Vec<_>>();
    let maximum_characters = maximum_characters.max(1);
    let mut parts = Vec::new();
    let mut start = 0usize;

    while start < characters.len() {
        let hard_end = characters
            .len()
            .min(start.saturating_add(maximum_characters));
        let mut end = hard_end;
        if hard_end < characters.len() {
            let earliest_break = start.saturating_add(maximum_characters.saturating_mul(7) / 10);
            if let Some(relative_end) = characters[earliest_break..hard_end]
                .iter()
                .rposition(|character| is_text_break(*character))
            {
                end = earliest_break
                    .saturating_add(relative_end)
                    .saturating_add(1);
            }
            if is_forbidden_line_start(characters[end]) && end > start + 1 {
                end -= 1;
            }
        }
        let part = characters[start..end]
            .iter()
            .collect::<String>()
            .trim()
            .to_string();
        if !part.is_empty() {
            parts.push(part);
        }
        start = end;
        while start < characters.len() && characters[start].is_whitespace() {
            start += 1;
        }
    }
    parts
}

fn is_text_break(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '。' | '！' | '？' | '；' | '，' | '、' | '：' | '.' | '!' | '?' | ';' | ',' | ':'
        )
}

fn is_forbidden_line_start(character: char) -> bool {
    matches!(
        character,
        '。' | '！' | '？' | '；' | '，' | '、' | '：' | '.' | '!' | '?' | ';' | ',' | ':'
    )
}

pub fn validate_cues(cues: &[SubtitleCue], policy: CuePolicy) -> Result<SubtitleQaReport> {
    if cues.is_empty() {
        bail!("subtitle QA failed: no cues were generated");
    }

    let mut warnings = Vec::new();
    let mut previous_end_cs = None;
    let mut previous_text = None::<String>;

    for (position, cue) in cues.iter().enumerate() {
        let expected_index = position + 1;
        if cue.index != expected_index {
            bail!(
                "subtitle QA failed: cue index {} should be {expected_index}",
                cue.index
            );
        }
        if cue.text.trim().is_empty() {
            bail!("subtitle QA failed: cue {} has empty text", cue.index);
        }
        if cue.start_cs < 0 || cue.end_cs <= cue.start_cs {
            bail!("subtitle QA failed: cue {} has invalid timing", cue.index);
        }
        if previous_end_cs.is_some_and(|end_cs| cue.start_cs < end_cs) {
            bail!(
                "subtitle QA failed: cue {} overlaps the previous cue",
                cue.index
            );
        }

        let duration_cs = cue.end_cs.saturating_sub(cue.start_cs);
        if duration_cs < policy.min_duration_cs {
            warnings.push(format!(
                "cue {} lasts {:.1}s, below the configured {:.1}s minimum because the next cue starts too soon",
                cue.index,
                duration_cs as f64 / 100.0,
                policy.min_duration_cs as f64 / 100.0
            ));
        }
        if duration_cs > policy.max_duration_cs {
            warnings.push(format!(
                "cue {} lasts {:.1}s, above the configured {:.1}s maximum",
                cue.index,
                duration_cs as f64 / 100.0,
                policy.max_duration_cs as f64 / 100.0
            ));
        }

        let lines = cue.text.lines().collect::<Vec<_>>();
        if lines.len() > policy.max_lines {
            warnings.push(format!(
                "cue {} has {} lines, above the configured {}-line maximum",
                cue.index,
                lines.len(),
                policy.max_lines
            ));
        }
        for line in lines {
            let character_count = line.chars().count();
            let limit = if line.chars().any(is_cjk_character) {
                policy.max_cjk_chars_per_line
            } else {
                policy.max_chars_per_line
            };
            if character_count > limit {
                warnings.push(format!(
                    "cue {} has a {}-character line, above the configured {}-character limit",
                    cue.index, character_count, limit
                ));
            }
        }

        let comparable_text = comparable_cue_text(&cue.text);
        if !comparable_text.is_empty()
            && previous_text
                .as_deref()
                .is_some_and(|text| text == comparable_text)
        {
            warnings.push(format!("cue {} repeats the previous cue text", cue.index));
        }
        previous_text = Some(comparable_text);
        previous_end_cs = Some(cue.end_cs);
    }

    Ok(SubtitleQaReport { warnings })
}

pub fn write_transcript_srt_file(path: &Path, cues: &[SubtitleCue]) -> Result<()> {
    let cues = cues
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
    write_srt_file(path, &cues)
}

pub fn write_srt_file(path: &Path, cues: &[SrtCue]) -> Result<()> {
    let content = render_srt(cues);
    atomic_write(path, content)
        .with_context(|| format!("failed to write subtitle file {}", path.display()))
}

pub fn parse_srt_file(path: &Path) -> Result<Vec<SrtCue>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read subtitle file {}", path.display()))?;
    parse_srt(&raw)
}

fn render_srt(cues: &[SrtCue]) -> String {
    let mut output = String::new();
    for cue in cues {
        let text = normalize_cue_text(&cue.text);
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            cue.index,
            format_timestamp(cue.start_ms),
            format_timestamp(cue.end_ms),
            text
        ));
    }
    output
}

fn normalize_cue_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn comparable_cue_text(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric() || is_cjk_character(*character))
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_cjk_character(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{3040}'..='\u{30ff}'
            | '\u{ac00}'..='\u{d7af}'
    )
}

fn parse_srt(raw: &str) -> Result<Vec<SrtCue>> {
    let mut cues = Vec::new();
    let normalized = raw
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");

    for chunk in srt_blocks(&normalized) {
        let mut lines = chunk.lines();
        let index = lines
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing cue index"))?
            .trim()
            .parse::<usize>()
            .context("invalid cue index")?;
        let timing = lines
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing cue timing"))?
            .trim();
        let (start, end) = timing
            .split_once("-->")
            .ok_or_else(|| anyhow::anyhow!("invalid cue timing: {timing}"))?;
        let text = lines.collect::<Vec<_>>().join("\n").trim().to_string();
        if text.is_empty() {
            bail!("cue {index} is empty");
        }

        let start_ms = parse_timestamp(start)?;
        let end_ms = parse_timestamp(end)?;
        if end_ms < start_ms {
            bail!("cue {index} ends before it starts");
        }

        cues.push(SrtCue {
            index,
            start_ms,
            end_ms,
            text,
        });
    }

    Ok(cues)
}

fn srt_blocks(normalized: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = Vec::new();

    for line in normalized.lines() {
        if line.trim().is_empty() {
            if !lines.is_empty() {
                blocks.push(lines.join("\n"));
                lines.clear();
            }
        } else {
            lines.push(line);
        }
    }
    if !lines.is_empty() {
        blocks.push(lines.join("\n"));
    }

    blocks
}

fn format_timestamp(total_millis: i64) -> String {
    let hours = total_millis / 3_600_000;
    let minutes = (total_millis % 3_600_000) / 60_000;
    let seconds = (total_millis % 60_000) / 1_000;
    let millis = total_millis % 1_000;

    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

fn parse_timestamp(input: &str) -> Result<i64> {
    let input = input.trim();
    let (hms, millis) = input
        .split_once(',')
        .or_else(|| input.split_once('.'))
        .ok_or_else(|| anyhow::anyhow!("invalid subtitle timestamp: {input}"))?;
    let mut parts = hms.split(':');
    let hours = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid subtitle timestamp: {input}"))?
        .parse::<i64>()?;
    let minutes = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid subtitle timestamp: {input}"))?
        .parse::<i64>()?;
    let seconds = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid subtitle timestamp: {input}"))?
        .parse::<i64>()?;
    if parts.next().is_some()
        || hours < 0
        || !(0..60).contains(&minutes)
        || !(0..60).contains(&seconds)
    {
        bail!("invalid subtitle timestamp: {input}");
    }
    let millis = millis.parse::<i64>()?;
    if !(0..1000).contains(&millis) {
        bail!("invalid subtitle timestamp: {input}");
    }

    let total_millis = hours
        .checked_mul(3_600_000)
        .and_then(|value| value.checked_add(minutes * 60_000))
        .and_then(|value| value.checked_add(seconds * 1_000))
        .and_then(|value| value.checked_add(millis))
        .ok_or_else(|| anyhow::anyhow!("subtitle timestamp is too large: {input}"))?;
    Ok(total_millis)
}

#[cfg(test)]
mod tests {
    use super::{
        CuePolicy, SrtCue, SubtitleCue, TimedWord, build_cues_from_timed_words, format_timestamp,
        optimize_cues, parse_srt, parse_timestamp, render_srt, validate_cues,
    };

    fn cue_policy() -> CuePolicy {
        CuePolicy {
            min_duration_cs: 80,
            max_duration_cs: 600,
            end_padding_cs: 15,
            max_chars_per_line: 42,
            max_cjk_chars_per_line: 18,
            max_lines: 2,
        }
    }

    #[test]
    fn formats_srt_timestamp() {
        assert_eq!(format_timestamp(1239), "00:00:01,239");
    }

    #[test]
    fn renders_srt_content() {
        let content = render_srt(&[SrtCue {
            index: 1,
            start_ms: 0,
            end_ms: 1230,
            text: "hello".to_string(),
        }]);
        assert!(content.contains("00:00:00,000 --> 00:00:01,230"));
        assert!(content.contains("hello"));
    }

    #[test]
    fn rendering_removes_blank_lines_inside_cues() {
        let content = render_srt(&[SrtCue {
            index: 1,
            start_ms: 0,
            end_ms: 1000,
            text: " first line \n\n  \n second line ".to_string(),
        }]);
        assert!(content.contains("first line\nsecond line\n\n"));
        assert!(!content.contains("first line\n\nsecond line"));
    }

    #[test]
    fn parses_srt_content() {
        let cues =
            parse_srt("1\n00:00:00,000 --> 00:00:01,230\nhello\n").expect("srt should parse");
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "hello");
        assert_eq!(cues[0].end_ms, 1230);
    }

    #[test]
    fn rewriting_srt_text_preserves_millisecond_timing_and_indexes() {
        let source = "7\n00:00:01,231 --> 00:00:01,239\nsource\n\n8\n00:00:01,241 --> 00:00:02,987\nsource\n\n";
        let mut cues = parse_srt(source).expect("SRT should parse");
        for cue in &mut cues {
            cue.text = "translated".to_string();
        }

        assert_eq!(render_srt(&cues), source.replace("source", "translated"));
    }

    #[test]
    fn parses_crlf_bom_and_flexible_arrow_spacing() {
        let cues = parse_srt(
            "\u{feff}1\r\n00:00:00,000-->00:00:01,230\r\nhello\r\n\r\n2\r\n00:00:01.230 --> 00:00:02.000\r\nworld\r\n",
        )
        .expect("common SRT variants should parse");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "hello");
        assert_eq!(cues[1].start_ms, 1230);
    }

    #[test]
    fn parses_multiple_and_whitespace_only_blank_lines() {
        let cues = parse_srt(
            "1\n00:00:00,000 --> 00:00:01,000\nhello\n\n \n\n2\n00:00:01,000 --> 00:00:02,000\nworld\n",
        )
        .expect("extra blank lines should parse");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[1].text, "world");
    }

    #[test]
    fn rejects_reversed_cue_timing() {
        let error = parse_srt("1\n00:00:02,000 --> 00:00:01,000\nhello\n")
            .expect_err("reversed cue should be rejected");
        assert!(error.to_string().contains("ends before it starts"));
    }

    #[test]
    fn parses_timestamp() {
        assert_eq!(
            parse_timestamp("00:00:01,230").expect("timestamp should parse"),
            1230
        );
    }

    #[test]
    fn rejects_timestamp_overflow() {
        assert!(parse_timestamp("9223372036854775807:00:00,000").is_err());
    }

    #[test]
    fn optimizer_normalizes_timing_and_indexes() {
        let optimized = optimize_cues(
            vec![
                SubtitleCue {
                    index: 9,
                    start_cs: 200,
                    end_cs: 210,
                    text: " second ".to_string(),
                },
                SubtitleCue {
                    index: 4,
                    start_cs: -20,
                    end_cs: 40,
                    text: "first".to_string(),
                },
            ],
            1_000,
            cue_policy(),
        );

        assert_eq!(optimized.len(), 2);
        assert_eq!(optimized[0].index, 1);
        assert_eq!(optimized[0].start_cs, 0);
        assert_eq!(optimized[0].end_cs, 80);
        assert_eq!(optimized[1].index, 2);
        assert_eq!(optimized[1].start_cs, 200);
        assert_eq!(optimized[1].end_cs, 280);
        assert_eq!(optimized[1].text, "second");
    }

    #[test]
    fn timed_words_split_cjk_at_punctuation() {
        let words = vec![
            TimedWord {
                start_cs: 10,
                end_cs: 80,
                text: "现在进入第二段，".to_string(),
            },
            TimedWord {
                start_cs: 80,
                end_cs: 160,
                text: "这是后半句。".to_string(),
            },
            TimedWord {
                start_cs: 220,
                end_cs: 300,
                text: "新的一句。".to_string(),
            },
        ];

        let cues = build_cues_from_timed_words(&words, cue_policy(), 50);

        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "现在进入第二段，这是后半句。");
        assert_eq!(cues[0].start_cs, 10);
        assert_eq!(cues[0].end_cs, 160);
        assert_eq!(cues[1].start_cs, 220);
    }

    #[test]
    fn timed_words_preserve_latin_spacing_and_split_on_pause() {
        let words = vec![
            TimedWord {
                start_cs: 0,
                end_cs: 50,
                text: " Hello".to_string(),
            },
            TimedWord {
                start_cs: 50,
                end_cs: 100,
                text: " world".to_string(),
            },
            TimedWord {
                start_cs: 180,
                end_cs: 240,
                text: " again".to_string(),
            },
        ];

        let cues = build_cues_from_timed_words(&words, cue_policy(), 50);

        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Hello world");
        assert_eq!(cues[1].text, "again");
    }

    #[test]
    fn timed_words_respect_cjk_character_limit_without_orphaning_punctuation() {
        let words = (0..5)
            .map(|index| TimedWord {
                start_cs: index * 100,
                end_cs: index * 100 + 100,
                text: if index == 4 {
                    "十三十四。".to_string()
                } else {
                    "一二三四".to_string()
                },
            })
            .collect::<Vec<_>>();

        let cues = build_cues_from_timed_words(&words, cue_policy(), 50);

        assert!(cues.len() >= 2);
        assert!(cues.iter().all(|cue| cue.text.chars().count() <= 18));
        assert!(cues.iter().all(|cue| !cue.text.starts_with('。')));
    }

    #[test]
    fn timed_words_keep_standalone_punctuation_with_preceding_text() {
        let words = vec![
            TimedWord {
                start_cs: 0,
                end_cs: 200,
                text: "一二三四五六七八九十一二三四五六".to_string(),
            },
            TimedWord {
                start_cs: 200,
                end_cs: 250,
                text: "七八".to_string(),
            },
            TimedWord {
                start_cs: 250,
                end_cs: 251,
                text: "。".to_string(),
            },
        ];

        let cues = build_cues_from_timed_words(&words, cue_policy(), 50);

        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text.chars().count(), 16);
        assert_eq!(cues[1].text, "七八。");
        assert!(cues.iter().all(|cue| !cue.text.starts_with('。')));
    }

    #[test]
    fn optimizer_caps_long_cues_and_prevents_overlaps() {
        let optimized = optimize_cues(
            vec![
                SubtitleCue {
                    index: 1,
                    start_cs: 100,
                    end_cs: 1_000,
                    text: "first".to_string(),
                },
                SubtitleCue {
                    index: 2,
                    start_cs: 450,
                    end_cs: 500,
                    text: "second".to_string(),
                },
            ],
            1_200,
            cue_policy(),
        );

        assert_eq!(optimized[0].end_cs, 450);
        assert_eq!(optimized[1].end_cs, 530);
        validate_cues(&optimized, cue_policy()).expect("optimized cues should pass QA");
    }

    #[test]
    fn optimizer_removes_blank_and_out_of_range_cues() {
        let optimized = optimize_cues(
            vec![
                SubtitleCue {
                    index: 1,
                    start_cs: 0,
                    end_cs: 100,
                    text: "  ".to_string(),
                },
                SubtitleCue {
                    index: 2,
                    start_cs: 1_000,
                    end_cs: 1_100,
                    text: "past the end".to_string(),
                },
            ],
            500,
            cue_policy(),
        );
        assert!(optimized.is_empty());
    }

    #[test]
    fn optimizer_splits_long_cjk_segments_across_their_full_duration() {
        let text = "短暂停顿应该被合并因为自然说话时经常需要换气或者思考但是比较长的安静区域不应该交给识别模型以免浪费时间或者产生不存在的文字";
        let optimized = optimize_cues(
            vec![SubtitleCue {
                index: 1,
                start_cs: 100,
                end_cs: 2_200,
                text: text.to_string(),
            }],
            3_000,
            cue_policy(),
        );

        assert!(optimized.len() >= 4);
        assert_eq!(optimized.first().expect("first cue").start_cs, 100);
        assert_eq!(optimized.last().expect("last cue").end_cs, 2_215);
        assert!(
            optimized
                .iter()
                .all(|cue| cue.end_cs.saturating_sub(cue.start_cs) <= 600)
        );
        assert!(optimized.iter().all(|cue| cue.text.lines().count() == 1));
        assert!(
            optimized
                .iter()
                .all(|cue| { cue.text.lines().all(|line| line.chars().count() <= 18) })
        );
        let reconstructed = optimized
            .iter()
            .flat_map(|cue| cue.text.chars())
            .filter(|character| *character != '\n')
            .collect::<String>();
        assert_eq!(reconstructed, text);
        validate_cues(&optimized, cue_policy()).expect("split cues should pass QA");
    }

    #[test]
    fn optimizer_preserves_long_duration_when_text_is_too_short_to_split_safely() {
        let optimized = optimize_cues(
            vec![SubtitleCue {
                index: 1,
                start_cs: 0,
                end_cs: 2_000,
                text: "hello".to_string(),
            }],
            3_000,
            cue_policy(),
        );

        assert_eq!(optimized.len(), 1);
        assert_eq!(optimized[0].end_cs, 2_015);
        let report =
            validate_cues(&optimized, cue_policy()).expect("long cue is structurally valid");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("above"))
        );
    }

    #[test]
    fn optimizer_preserves_all_text_when_timing_is_too_short_to_split() {
        let text =
            "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十";
        for media_duration_cs in [1, 1000] {
            let optimized = optimize_cues(
                vec![SubtitleCue {
                    index: 1,
                    start_cs: 0,
                    end_cs: 1,
                    text: text.to_string(),
                }],
                media_duration_cs,
                cue_policy(),
            );

            assert_eq!(optimized.len(), 1);
            assert_eq!(optimized[0].text, text);
            assert_eq!(optimized[0].start_cs, 0);
            assert_eq!(optimized[0].end_cs, media_duration_cs.min(80));
            let report = validate_cues(&optimized, cue_policy())
                .expect("the complete short cue should remain structurally valid");
            assert!(
                report
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("character"))
            );
        }
    }

    #[test]
    fn optimizer_does_not_start_a_wrapped_line_with_punctuation() {
        let optimized = optimize_cues(
            vec![SubtitleCue {
                index: 1,
                start_cs: 0,
                end_cs: 400,
                text: "一二三四五六七八九十一二三四五六七八。".to_string(),
            }],
            500,
            cue_policy(),
        );

        assert!(optimized.iter().all(|cue| !cue.text.starts_with('。')));
        assert!(optimized.iter().all(|cue| cue.text.chars().count() <= 18));
    }

    #[test]
    fn qa_rejects_overlaps_and_warns_about_readability() {
        let cues = vec![
            SubtitleCue {
                index: 1,
                start_cs: 0,
                end_cs: 100,
                text: "this line is deliberately much longer than forty two characters".to_string(),
            },
            SubtitleCue {
                index: 2,
                start_cs: 90,
                end_cs: 180,
                text: "second".to_string(),
            },
        ];
        let error = validate_cues(&cues, cue_policy()).expect_err("overlap should fail QA");
        assert!(error.to_string().contains("overlaps"));

        let non_overlapping = vec![cues[0].clone()];
        let report = validate_cues(&non_overlapping, cue_policy()).expect("cue should be valid");
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("character"));
    }

    #[test]
    fn qa_warns_when_spacing_forces_a_short_cue() {
        let cues = vec![SubtitleCue {
            index: 1,
            start_cs: 100,
            end_cs: 125,
            text: "short".to_string(),
        }];

        let report =
            validate_cues(&cues, cue_policy()).expect("short cue remains structurally valid");
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("below"));
    }
}
