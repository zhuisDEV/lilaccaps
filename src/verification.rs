use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::runtime::{FFPROBE_DEPENDENCY, ScopedTempPath, ensure_dependency, tmp_dir};

const MAX_METADATA_BYTES: usize = 128 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 8192;
const MAX_PACKET_LINE_BYTES: usize = 16 * 1024;
const PACKET_TIMESTAMP_TOLERANCE: f64 = 0.001;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f64,
    pub video_codec: String,
    pub video_stream_count: usize,
    pub frame_count: Option<u64>,
    pub frames_per_second: Option<f64>,
    pub audio_streams: Vec<AudioStreamMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStreamMetadata {
    pub index: u32,
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub channel_layout: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioVerification {
    pub source_stream_index: u32,
    pub output_stream_index: u32,
    pub packet_count: u64,
    pub packet_data_sha256_identical: bool,
    pub timestamps_preserved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub schema_version: u32,
    pub source: VideoMetadata,
    pub output: VideoMetadata,
    pub dimensions_preserved: bool,
    pub duration_preserved: bool,
    pub duration_tolerance_seconds: f64,
    pub frame_count_preserved: Option<bool>,
    pub audio: Vec<AudioVerification>,
    pub packet_timestamp_tolerance_seconds: f64,
    pub full_decode_passed: bool,
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct ProbeStream {
    index: u32,
    codec_type: String,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    nb_frames: Option<String>,
    avg_frame_rate: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    channel_layout: Option<String>,
}

/// Read container metadata without decoding the video or materialising packets.
pub fn probe_video(path: &Path) -> Result<VideoMetadata> {
    if !path.is_file() {
        bail!(
            "video input does not exist or is not a regular file: {}",
            path.display()
        );
    }
    ensure_dependency(FFPROBE_DEPENDENCY)?;
    let mut command = Command::new("ffprobe");
    command.args([
        "-v", "error", "-show_entries",
        "stream=index,codec_type,codec_name,width,height,nb_frames,avg_frame_rate,sample_rate,channels,channel_layout:format=duration",
        "-of", "json",
    ]).arg(path).stdin(Stdio::null()).stdout(Stdio::piped());
    let output = captured_metadata(&mut command)?;
    let probe: Probe = serde_json::from_slice(&output).with_context(|| {
        format!(
            "ffprobe returned invalid video metadata for {}",
            path.display()
        )
    })?;
    let videos: Vec<_> = probe
        .streams
        .iter()
        .filter(|stream| stream.codec_type == "video")
        .collect();
    let video = videos.first().context("media contains no video stream")?;
    let width = video
        .width
        .filter(|value| *value > 0)
        .context("video has no usable width")?;
    let height = video
        .height
        .filter(|value| *value > 0)
        .context("video has no usable height")?;
    let duration_seconds = probe
        .format
        .duration
        .as_deref()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .context("video has no finite positive duration")?;
    Ok(VideoMetadata {
        width,
        height,
        duration_seconds,
        video_codec: video
            .codec_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        video_stream_count: videos.len(),
        frame_count: video
            .nb_frames
            .as_deref()
            .and_then(|value| value.parse().ok()),
        frames_per_second: video.avg_frame_rate.as_deref().and_then(parse_rate),
        audio_streams: probe
            .streams
            .iter()
            .filter(|stream| stream.codec_type == "audio")
            .map(|stream| AudioStreamMetadata {
                index: stream.index,
                codec: stream
                    .codec_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                sample_rate: stream
                    .sample_rate
                    .as_deref()
                    .and_then(|value| value.parse().ok()),
                channels: stream.channels,
                channel_layout: stream.channel_layout.clone(),
            })
            .collect(),
    })
}

/// Verify a rendered candidate before publishing it. Packet manifests are temporary
/// files so verification uses bounded memory even for long, multi-track recordings.
pub fn verify_rendered_video(
    source: &Path,
    candidate: &Path,
    runtime_home: &Path,
) -> Result<VerificationReport> {
    let source_metadata = probe_video(source)?;
    let output_metadata = probe_video(candidate)?;
    if (source_metadata.width, source_metadata.height)
        != (output_metadata.width, output_metadata.height)
    {
        bail!(
            "render verification failed: video dimensions changed from {}x{} to {}x{}",
            source_metadata.width,
            source_metadata.height,
            output_metadata.width,
            output_metadata.height
        );
    }
    if output_metadata.video_stream_count != 1 {
        bail!(
            "render verification failed: expected one output video stream, found {}",
            output_metadata.video_stream_count
        );
    }
    let duration_tolerance = source_metadata
        .frames_per_second
        .map(|fps| 1.0 / fps)
        .unwrap_or(0.05)
        .max(0.05);
    if (source_metadata.duration_seconds - output_metadata.duration_seconds).abs()
        > duration_tolerance
    {
        bail!(
            "render verification failed: duration changed from {:.6}s to {:.6}s (allowed {:.6}s)",
            source_metadata.duration_seconds,
            output_metadata.duration_seconds,
            duration_tolerance
        );
    }
    let frame_count_preserved = match (source_metadata.frame_count, output_metadata.frame_count) {
        (Some(source), Some(output)) => {
            if source != output {
                bail!("render verification failed: frame count changed from {source} to {output}");
            }
            Some(true)
        }
        _ => None,
    };
    if source_metadata.audio_streams.len() != output_metadata.audio_streams.len() {
        bail!(
            "render verification failed: audio stream count changed from {} to {}",
            source_metadata.audio_streams.len(),
            output_metadata.audio_streams.len()
        );
    }
    let temporary = ScopedTempPath::directory(&tmp_dir(runtime_home), "verify-render")?;
    let mut audio = Vec::new();
    for (ordinal, (source_stream, output_stream)) in source_metadata
        .audio_streams
        .iter()
        .zip(&output_metadata.audio_streams)
        .enumerate()
    {
        if source_stream.codec != output_stream.codec
            || source_stream.sample_rate != output_stream.sample_rate
            || source_stream.channels != output_stream.channels
        {
            bail!(
                "render verification failed: audio stream {ordinal} codec, sample rate, or channel count changed"
            );
        }
        let source_manifest = temporary.path().join("source-audio.txt");
        let output_manifest = temporary.path().join("output-audio.txt");
        write_audio_packets(source, ordinal, &source_manifest)?;
        write_audio_packets(candidate, ordinal, &output_manifest)?;
        let packet_count = compare_audio_packets(&source_manifest, &output_manifest)
            .with_context(|| format!("render verification failed for audio stream {ordinal}"))?;
        audio.push(AudioVerification {
            source_stream_index: source_stream.index,
            output_stream_index: output_stream.index,
            packet_count,
            packet_data_sha256_identical: true,
            timestamps_preserved: true,
        });
    }
    let mut decode = Command::new("ffmpeg");
    decode
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-xerror",
            "-err_detect",
            "explode",
            "-i",
        ])
        .arg(candidate)
        .args(["-map", "0:v:0", "-map", "0:a?", "-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    checked_command(&mut decode, "render verification full decode")?;
    Ok(VerificationReport {
        schema_version: 1,
        source: source_metadata,
        output: output_metadata,
        dimensions_preserved: true,
        duration_preserved: true,
        duration_tolerance_seconds: duration_tolerance,
        frame_count_preserved,
        audio,
        packet_timestamp_tolerance_seconds: PACKET_TIMESTAMP_TOLERANCE,
        full_decode_passed: true,
    })
}

fn parse_rate(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let result = numerator.parse::<f64>().ok()? / denominator.parse::<f64>().ok()?;
    (result.is_finite() && result > 0.0).then_some(result)
}

fn write_audio_packets(video: &Path, ordinal: usize, manifest: &Path) -> Result<()> {
    let file =
        File::create(manifest).context("failed to create temporary audio verification manifest")?;
    let mut command = Command::new("ffprobe");
    command
        .args([
            "-v",
            "error",
            "-select_streams",
            &format!("a:{ordinal}"),
            "-show_packets",
            "-show_data_hash",
            "sha256",
            "-show_entries",
            "packet=pts_time,dts_time,duration_time,size,data_hash:packet_side_data=",
            "-of",
            "compact=p=0:nk=0",
        ])
        .arg(video)
        .stdin(Stdio::null())
        .stdout(Stdio::from(file));
    checked_command(&mut command, "audio packet verification probe")
}

#[derive(Debug)]
struct Packet {
    hash: String,
    size: u64,
    pts: Option<f64>,
    dts: Option<f64>,
    duration: Option<f64>,
}

fn read_packet(reader: &mut impl BufRead) -> Result<Option<Packet>> {
    loop {
        let mut bytes = Vec::new();
        let count = reader
            .take((MAX_PACKET_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if count == 0 {
            return Ok(None);
        }
        if count > MAX_PACKET_LINE_BYTES {
            bail!("audio verification packet metadata exceeded the size limit");
        }
        let line = std::str::from_utf8(&bytes)
            .context("audio verification packet metadata was not UTF-8")?;
        if line.trim().is_empty() {
            continue;
        }
        let fields: std::collections::HashMap<_, _> = line
            .trim()
            .split('|')
            .filter_map(|field| field.split_once('='))
            .collect();
        let hash = fields
            .get("data_hash")
            .context("audio packet is missing its SHA256 data hash")?
            .to_string();
        if !hash.starts_with("SHA256:")
            || hash.len() != 71
            || !hash[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("audio packet has an invalid SHA256 data hash");
        }
        let size = fields
            .get("size")
            .context("audio packet is missing its size")?
            .parse()
            .context("audio packet has an invalid size")?;
        let number = |name| -> Result<Option<f64>> {
            match fields.get(name).copied() {
                None | Some("N/A") => Ok(None),
                Some(value) => Ok(Some(
                    value
                        .parse::<f64>()
                        .ok()
                        .filter(|value| value.is_finite())
                        .with_context(|| format!("audio packet has invalid {name}"))?,
                )),
            }
        };
        return Ok(Some(Packet {
            hash,
            size,
            pts: number("pts_time")?,
            dts: number("dts_time")?,
            duration: number("duration_time")?,
        }));
    }
}

fn compare_audio_packets(source: &Path, candidate: &Path) -> Result<u64> {
    let mut source = BufReader::new(File::open(source)?);
    let mut candidate = BufReader::new(File::open(candidate)?);
    let mut count = 0;
    loop {
        match (read_packet(&mut source)?, read_packet(&mut candidate)?) {
            (None, None) => return Ok(count),
            (Some(source), Some(candidate)) => {
                count += 1;
                if source.hash != candidate.hash || source.size != candidate.size {
                    bail!(
                        "audio packet {count} data changed; audio must be copied without re-encoding"
                    );
                }
                for (label, source, candidate) in [
                    ("PTS", source.pts, candidate.pts),
                    ("DTS", source.dts, candidate.dts),
                    ("duration", source.duration, candidate.duration),
                ] {
                    let equal = match (source, candidate) {
                        (None, None) => true,
                        (Some(source), Some(candidate)) => {
                            (source - candidate).abs() <= PACKET_TIMESTAMP_TOLERANCE + f64::EPSILON
                        }
                        _ => false,
                    };
                    if !equal {
                        bail!("audio packet {count} {label} changed");
                    }
                }
            }
            _ => bail!("audio packet count changed after packet {count}"),
        }
    }
}

fn read_bounded(mut reader: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut truncated = false;
    let mut buffer = [0; 4096];
    loop {
        let length = reader.read(&mut buffer)?;
        if length == 0 {
            return Ok((output, truncated));
        }
        let kept = length.min(limit.saturating_sub(output.len()));
        output.extend_from_slice(&buffer[..kept]);
        truncated |= kept < length;
    }
}

fn diagnostic(bytes: &[u8], truncated: bool) -> String {
    let text = String::from_utf8_lossy(bytes);
    format!(
        "{}{}",
        text.trim(),
        if truncated {
            " [diagnostic truncated]"
        } else {
            ""
        }
    )
}

fn checked_command(command: &mut Command, operation: &str) -> Result<()> {
    let mut child = command
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start {operation}"))?;
    let stderr = child
        .stderr
        .take()
        .context("failed to capture media diagnostic")?;
    let reader = thread::spawn(move || read_bounded(stderr, MAX_DIAGNOSTIC_BYTES));
    let status = child
        .wait()
        .with_context(|| format!("failed to wait for {operation}"))?;
    let (stderr, truncated) = reader
        .join()
        .map_err(|_| anyhow::anyhow!("media diagnostic reader failed"))??;
    if !status.success() {
        bail!("{operation} failed: {}", diagnostic(&stderr, truncated));
    }
    Ok(())
}

fn captured_metadata(command: &mut Command) -> Result<Vec<u8>> {
    let mut child = command
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start video metadata probe")?;
    let stdout = child
        .stdout
        .take()
        .context("failed to capture video metadata")?;
    let stderr = child
        .stderr
        .take()
        .context("failed to capture video probe diagnostic")?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout, MAX_METADATA_BYTES));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_DIAGNOSTIC_BYTES));
    let status = child
        .wait()
        .context("failed to wait for video metadata probe")?;
    let (stdout, stdout_truncated) = stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("video metadata reader failed"))??;
    let (stderr, stderr_truncated) = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("video diagnostic reader failed"))??;
    if !status.success() {
        bail!(
            "video metadata probe failed: {}",
            diagnostic(&stderr, stderr_truncated)
        );
    }
    if stdout_truncated {
        bail!("video metadata exceeded the {MAX_METADATA_BYTES} byte limit");
    }
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn rejects_missing_or_malformed_audio_hashes() {
        assert!(read_packet(&mut Cursor::new(b"size=42|data_hash=\n")).is_err());
        assert!(read_packet(&mut Cursor::new(b"size=42\n")).is_err());
        let oversized = vec![b'x'; MAX_PACKET_LINE_BYTES + 1];
        assert!(read_packet(&mut Cursor::new(oversized)).is_err());
    }

    #[test]
    fn drains_output_but_keeps_memory_bounded() {
        let (output, truncated) = read_bounded(Cursor::new(vec![b'a'; 100_000]), 100).unwrap();
        assert_eq!(output.len(), 100);
        assert!(truncated);
    }

    #[test]
    fn validates_frame_rates() {
        assert_eq!(parse_rate("30000/1001"), Some(30000.0 / 1001.0));
        assert!(parse_rate("0/0").is_none());
        assert!(parse_rate("-1/1").is_none());
    }
}
