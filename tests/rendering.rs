mod common;

use common::Fixture;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn success(command: &mut Command) -> Output {
    let output = command.output().expect("required media tool should start");
    assert!(
        output.status.success(),
        "{command:?}\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

fn video_fixture() -> Fixture {
    let fixture = Fixture::new();
    success(
        Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=gray:s=640x360:r=100:d=0.4",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(fixture.0.join("input.mp4")),
    );
    fs::write(fixture.0.join("config.toml"), format!(
        "[runtime]\nhome = {:?}\n[agent]\nskill_path = {:?}\n[release]\n[transcribe.model]\nid = \"base\"\n",
        fixture.0.join("runtime"), fixture.0.join("SKILL.md"),
    )).unwrap();
    fs::write(
        fixture.0.join("input.srt"),
        "1\n00:00:00,000 --> 00:00:00,400\nHello World\n",
    )
    .unwrap();
    fixture
}

fn burnin(fixture: &Fixture, subtitles: &Path, output: &str, options: &[&str]) -> PathBuf {
    let path = fixture.0.join(output);
    success(
        fixture
            .command()
            .arg("burnin")
            .arg(fixture.0.join("input.mp4"))
            .arg("--subs")
            .arg(subtitles)
            .arg("--config-path")
            .arg(fixture.0.join("config.toml"))
            .arg("--output")
            .arg(&path)
            .args(options),
    );
    let probe = success(
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=p=0:s=x",
            ])
            .arg(&path),
    );
    assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), "640x360");
    path
}

fn frame_digest(path: &Path, timestamp: &str) -> Vec<u8> {
    success(
        Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(path)
            .args(["-ss", timestamp, "-frames:v", "1", "-f", "md5", "-"]),
    )
    .stdout
}

fn reference(fixture: &Fixture, filter: &str) -> PathBuf {
    let output = fixture.0.join("reference.mp4");
    success(
        Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(fixture.0.join("input.mp4"))
            .args(["-vf", filter, "-c:v", "libx264", "-pix_fmt", "yuv420p"])
            .arg(&output),
    );
    output
}

#[test]
fn special_characters_in_subtitle_paths_preserve_the_rendered_frames() {
    let fixture = video_fixture();
    let plain = fixture.0.join("input.srt");
    let special = fixture.0.join("Brian's [cut],semi;colon:back\\slash.srt");
    fs::copy(&plain, &special).unwrap();
    let expected = burnin(&fixture, &plain, "plain.mp4", &[]);
    let actual = burnin(&fixture, &special, "special.mp4", &[]);
    assert_eq!(frame_digest(&actual, "0"), frame_digest(&expected, "0"));
}

#[test]
fn watermark_text_matches_a_literal_textfile_reference() {
    let fixture = video_fixture();
    let text = r"it's 50%: [ready], C:\temp; = done";
    let textfile = fixture.0.join("literal.txt");
    fs::write(&textfile, text).unwrap();
    let actual = fixture.0.join("watermark.mp4");
    let report = success(
        fixture
            .command()
            .arg("watermark")
            .arg(fixture.0.join("input.mp4"))
            .args([
                "--text",
                text,
                "--opacity",
                "1",
                "--position",
                "top-left",
                "--size",
                "18",
                "--margin",
                "10",
            ])
            .arg("--output")
            .arg(&actual),
    );
    assert!(String::from_utf8_lossy(&report.stdout).contains("renderer = ffmpeg-drawtext"));
    let expected = reference(
        &fixture,
        &format!(
            "drawtext=textfile={}:expansion=none:x=10:y=10:fontsize=18:fontcolor=white@1.000:shadowcolor=black@1.000:shadowx=2:shadowy=2",
            textfile.display(),
        ),
    );
    assert_eq!(frame_digest(&actual, "0"), frame_digest(&expected, "0"));
}

#[test]
fn no_outline_matches_an_explicit_zero_outline_reference() {
    let fixture = video_fixture();
    let subtitles = fixture.0.join("input.srt");
    let actual = burnin(&fixture, &subtitles, "no-outline.mp4", &["--no-outline"]);
    let default = burnin(&fixture, &subtitles, "default.mp4", &[]);
    let expected = reference(
        &fixture,
        &format!(
            "subtitles=filename={}:force_style='BorderStyle=1,Outline=0'",
            subtitles.display(),
        ),
    );
    assert_eq!(frame_digest(&actual, "0"), frame_digest(&expected, "0"));
    assert_ne!(frame_digest(&actual, "0"), frame_digest(&default, "0"));
}

#[test]
fn invalid_unicode_colour_fails_without_replacing_an_existing_output() {
    let fixture = video_fixture();
    let destination = fixture.0.join("existing.mp4");
    fs::write(&destination, "existing output").unwrap();
    let result = fixture
        .command()
        .arg("burnin")
        .arg(fixture.0.join("input.mp4"))
        .arg("--subs")
        .arg(fixture.0.join("input.srt"))
        .arg("--config-path")
        .arg(fixture.0.join("config.toml"))
        .args(["--outline-colour", "#a€bb"])
        .arg("--output")
        .arg(&destination)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
    assert_eq!(fs::read_to_string(&destination).unwrap(), "existing output");
}

#[test]
fn overlay_rendering_preserves_imported_millisecond_boundaries() {
    let fixture = video_fixture();
    let subtitles = fixture.0.join("precise.srt");
    fs::write(&subtitles, "1\n00:00:00,239 --> 00:00:00,350\nHello\n").unwrap();
    let output = burnin(&fixture, &subtitles, "overlay.mp4", &["--colour", "white"]);
    let input = fixture.0.join("input.mp4");
    assert_eq!(
        frame_digest(&output, "0.230"),
        frame_digest(&input, "0.230")
    );
    assert_ne!(
        frame_digest(&output, "0.240"),
        frame_digest(&input, "0.240")
    );
}
