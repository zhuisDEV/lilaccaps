use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct SubtitleCue {
    pub index: usize,
    pub start_cs: i64,
    pub end_cs: i64,
    pub text: String,
}

pub fn write_srt_file(path: &Path, cues: &[SubtitleCue]) -> Result<()> {
    let content = render_srt(cues);
    fs::write(path, content)
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
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            cue.index,
            format_timestamp(cue.start_cs),
            format_timestamp(cue.end_cs),
            cue.text.trim()
        ));
    }
    output
}

fn parse_srt(raw: &str) -> Result<Vec<SubtitleCue>> {
    let mut cues = Vec::new();

    for chunk in raw.split("\n\n").filter(|item| !item.trim().is_empty()) {
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
            .split_once(" --> ")
            .ok_or_else(|| anyhow::anyhow!("invalid cue timing: {timing}"))?;
        let text = lines.collect::<Vec<_>>().join("\n").trim().to_string();
        if text.is_empty() {
            bail!("cue {index} is empty");
        }

        cues.push(SubtitleCue {
            index,
            start_cs: parse_timestamp(start)?,
            end_cs: parse_timestamp(end)?,
            text,
        });
    }

    Ok(cues)
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
    let (hms, millis) = input
        .trim()
        .split_once(',')
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
    let millis = millis.parse::<i64>()?;

    Ok(hours * 360_000 + minutes * 6_000 + seconds * 100 + millis / 10)
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
    fn parses_srt_content() {
        let cues =
            parse_srt("1\n00:00:00,000 --> 00:00:01,230\nhello\n").expect("srt should parse");
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "hello");
        assert_eq!(cues[0].end_cs, 123);
    }

    #[test]
    fn parses_timestamp() {
        assert_eq!(
            parse_timestamp("00:00:01,230").expect("timestamp should parse"),
            123
        );
    }
}
