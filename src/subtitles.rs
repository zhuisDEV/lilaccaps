use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::runtime::atomic_write;

#[derive(Debug, Clone)]
pub struct SubtitleCue {
    pub index: usize,
    pub start_cs: i64,
    pub end_cs: i64,
    pub text: String,
}

pub fn write_srt_file(path: &Path, cues: &[SubtitleCue]) -> Result<()> {
    let content = render_srt(cues);
    atomic_write(path, content)
        .with_context(|| format!("failed to write subtitle file {}", path.display()))
}

pub fn parse_srt_file(path: &Path) -> Result<Vec<SubtitleCue>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read subtitle file {}", path.display()))?;
    parse_srt(&raw)
}

fn render_srt(cues: &[SubtitleCue]) -> String {
    let mut output = String::new();
    for cue in cues {
        let text = normalize_cue_text(&cue.text);
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            cue.index,
            format_timestamp(cue.start_cs),
            format_timestamp(cue.end_cs),
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

fn parse_srt(raw: &str) -> Result<Vec<SubtitleCue>> {
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

        let start_cs = parse_timestamp(start)?;
        let end_cs = parse_timestamp(end)?;
        if end_cs < start_cs {
            bail!("cue {index} ends before it starts");
        }

        cues.push(SubtitleCue {
            index,
            start_cs,
            end_cs,
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

fn format_timestamp(centiseconds: i64) -> String {
    let total_millis = centiseconds * 10;
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
    Ok(total_millis / 10)
}

#[cfg(test)]
mod tests {
    use super::{SubtitleCue, format_timestamp, parse_srt, parse_timestamp, render_srt};

    #[test]
    fn formats_srt_timestamp() {
        assert_eq!(format_timestamp(123), "00:00:01,230");
    }

    #[test]
    fn renders_srt_content() {
        let content = render_srt(&[SubtitleCue {
            index: 1,
            start_cs: 0,
            end_cs: 123,
            text: "hello".to_string(),
        }]);
        assert!(content.contains("00:00:00,000 --> 00:00:01,230"));
        assert!(content.contains("hello"));
    }

    #[test]
    fn rendering_removes_blank_lines_inside_cues() {
        let content = render_srt(&[SubtitleCue {
            index: 1,
            start_cs: 0,
            end_cs: 100,
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
        assert_eq!(cues[0].end_cs, 123);
    }

    #[test]
    fn parses_crlf_bom_and_flexible_arrow_spacing() {
        let cues = parse_srt(
            "\u{feff}1\r\n00:00:00,000-->00:00:01,230\r\nhello\r\n\r\n2\r\n00:00:01.230 --> 00:00:02.000\r\nworld\r\n",
        )
        .expect("common SRT variants should parse");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "hello");
        assert_eq!(cues[1].start_cs, 123);
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
            123
        );
    }

    #[test]
    fn rejects_timestamp_overflow() {
        assert!(parse_timestamp("9223372036854775807:00:00,000").is_err());
    }
}
