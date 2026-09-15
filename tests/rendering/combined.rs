use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use super::{BurninStyle, OutlineStyle, burn_in_subtitles, burn_in_subtitles_with_watermark};
use crate::media::subtitles_filter;
use crate::runtime::ScopedTempPath;
use crate::verification::{probe_video, verify_rendered_video};
use crate::watermark::{WatermarkPosition, WatermarkSource, WatermarkStyle, text_filter};

fn success(command: &mut Command) -> Output {
    let output = command.output().expect("media dependency should start");
    assert!(
        output.status.success(),
        "{command:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn fixture(audio_tracks: usize) -> ScopedTempPath {
    let fixture =
        ScopedTempPath::directory(&std::env::temp_dir(), "lilaccaps-combined-test").unwrap();
    let mut command = Command::new("ffmpeg");
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=gray:s=640x360:r=25:d=1.2",
    ]);
    for frequency in [440, 880].iter().take(audio_tracks) {
        command.args([
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={frequency}:sample_rate=48000:duration=1.2"),
        ]);
    }
    command.args(["-map", "0:v:0"]);
    for index in 0..audio_tracks {
        command.args(["-map", &format!("{}:a:0", index + 1)]);
    }
    success(
        command
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"])
            .arg(fixture.path().join("input.mp4")),
    );
    fs::write(
        fixture.path().join("captions.srt"),
        "1\n00:00:00,000 --> 00:00:01,200\nHello 世界\n",
    )
    .unwrap();
    fixture
}

fn caption_style() -> BurninStyle {
    BurninStyle {
        font: None,
        colour: None,
        size: Some(20),
        line_spacing: None,
        outline: OutlineStyle {
            enabled: true,
            colour: Some("black".to_string()),
            width: 2,
        },
        line_order: Vec::new(),
        line_styles: HashMap::new(),
    }
}

fn watermark_style() -> WatermarkStyle {
    WatermarkStyle {
        position: WatermarkPosition::TopRight,
        opacity: 0.7,
        size: 24,
        margin: 12,
        colour: "white".to_string(),
        font: None,
        outline_colour: "black".to_string(),
        outline_width: 0,
    }
}

fn first_frame_hash(video: &Path, crop: Option<&str>) -> Vec<u8> {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(video)
        .args(["-map", "0:v:0", "-frames:v", "1"]);
    if let Some(crop) = crop {
        command.args(["-vf", crop]);
    }
    success(command.args(["-f", "hash", "-hash", "sha256", "-"])).stdout
}

#[test]
fn native_caption_and_text_watermark_match_one_encode_and_preserve_all_audio() {
    let fixture = fixture(2);
    let input = fixture.path().join("input.mp4");
    let captions = fixture.path().join("captions.srt");
    let output = fixture.path().join("combined.mp4");
    let reference = fixture.path().join("reference.mp4");
    let caption_style = caption_style();
    let watermark_style = watermark_style();
    let source = WatermarkSource::Text("LILAC".to_string());
    let report = burn_in_subtitles_with_watermark(
        fixture.path(),
        &input,
        &captions,
        &output,
        &caption_style,
        Some((&source, &watermark_style)),
    )
    .unwrap();
    assert_eq!(report.renderer, "ffmpeg-subtitles+watermark");
    let filter = format!(
        "{},{}",
        subtitles_filter(&captions, None, Some(20), Some("black"), Some(2)),
        text_filter("LILAC", &watermark_style)
    );
    success(
        Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(&input)
            .args([
                "-vf", &filter, "-map", "0:v:0", "-map", "0:a?", "-c:v", "libx264", "-pix_fmt",
                "yuv420p", "-c:a", "copy",
            ])
            .arg(reference.clone()),
    );
    assert_eq!(
        first_frame_hash(&output, None),
        first_frame_hash(&reference, None)
    );
    let verified = verify_rendered_video(&input, &output, fixture.path()).unwrap();
    assert!(verified.full_decode_passed);
    assert_eq!(verified.audio.len(), 2);
    assert!(
        verified
            .audio
            .iter()
            .all(|audio| audio.packet_count > 1 && audio.packet_data_sha256_identical)
    );
    assert_eq!(verified.frame_count_preserved, Some(true));
    assert!((probe_video(&input).unwrap().duration_seconds - 1.2).abs() < 0.05);
    assert!(
        fs::read_dir(fixture.path().join("tmp"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn advanced_captions_and_image_watermark_render_together_without_audio() {
    let fixture = fixture(0);
    let input = fixture.path().join("input.mp4");
    let captions = fixture.path().join("captions.srt");
    let plain = fixture.path().join("plain.mp4");
    let output = fixture.path().join("combined.mp4");
    let image = fixture.path().join("watermark.png");
    success(
        Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=80x40",
                "-frames:v",
                "1",
                "-threads",
                "1",
            ])
            .arg(&image),
    );
    let mut captions_style = caption_style();
    captions_style.colour = Some("yellow".to_string());
    captions_style.font = Some("sans-serif".to_string());
    let mut style = watermark_style();
    style.size = 80;
    style.opacity = 1.0;
    burn_in_subtitles(fixture.path(), &input, &captions, &plain, &captions_style).unwrap();
    let report = burn_in_subtitles_with_watermark(
        fixture.path(),
        &input,
        &captions,
        &output,
        &captions_style,
        Some((&WatermarkSource::Image(image), &style)),
    )
    .unwrap();
    assert_eq!(report.renderer, "overlay-fallback+watermark");
    assert!(report.reasons.contains(&"colour"));
    assert_ne!(
        first_frame_hash(&output, Some("crop=120:80:520:0")),
        first_frame_hash(&plain, Some("crop=120:80:520:0"))
    );
    assert_ne!(
        first_frame_hash(&output, Some("crop=640:120:0:240")),
        first_frame_hash(&input, Some("crop=640:120:0:240"))
    );
    let verified = verify_rendered_video(&input, &output, fixture.path()).unwrap();
    assert!(verified.audio.is_empty());
    assert!(verified.full_decode_passed);
}

#[test]
fn native_captions_support_svg_watermarks_and_text_image_fallback() {
    let fixture = fixture(0);
    let input = fixture.path().join("input.mp4");
    let captions = fixture.path().join("captions.srt");
    let svg = fixture.path().join("watermark.svg");
    fs::write(&svg, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"80\" height=\"40\"><rect width=\"80\" height=\"40\" fill=\"red\"/></svg>").unwrap();
    let style = watermark_style();
    let output = fixture.path().join("svg.mp4");
    burn_in_subtitles_with_watermark(
        fixture.path(),
        &input,
        &captions,
        &output,
        &caption_style(),
        Some((&WatermarkSource::Image(svg), &style)),
    )
    .unwrap();
    assert!(
        verify_rendered_video(&input, &output, fixture.path())
            .unwrap()
            .full_decode_passed
    );
    let source = WatermarkSource::Text("中文字 LILAC".to_string());
    let prepared = super::prepare_watermark(&source, &style, fixture.path(), true).unwrap();
    let fallback = fixture.path().join("text-fallback.mp4");
    let subtitles = subtitles_filter(&captions, None, Some(20), Some("black"), Some(2));
    super::render_composite(&input, &fallback, Some(&subtitles), &[], Some(&prepared)).unwrap();
    assert!(
        verify_rendered_video(&input, &fallback, fixture.path())
            .unwrap()
            .full_decode_passed
    );
    assert_ne!(
        first_frame_hash(&fallback, Some("crop=300:80:340:0")),
        first_frame_hash(&input, Some("crop=300:80:340:0"))
    );
}

#[test]
fn verification_rejects_missing_and_reencoded_audio() {
    let fixture = fixture(1);
    let input = fixture.path().join("input.mp4");
    let no_audio = fixture.path().join("silent.mp4");
    success(
        Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(&input)
            .args(["-an", "-c:v", "copy"])
            .arg(&no_audio),
    );
    let error = verify_rendered_video(&input, &no_audio, fixture.path()).unwrap_err();
    assert!(error.to_string().contains("audio stream count changed"));
    let reencoded = fixture.path().join("reencoded.mp4");
    success(
        Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(&input)
            .args(["-c:v", "copy", "-c:a", "aac", "-b:a", "32k"])
            .arg(&reencoded),
    );
    let error = verify_rendered_video(&input, &reencoded, fixture.path()).unwrap_err();
    assert!(format!("{error:#}").contains("audio packet 1 data changed"));
}
