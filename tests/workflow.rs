#![cfg(unix)]
mod common;

use common::Fixture;
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture() -> Fixture {
    let fixture = Fixture::new();
    let result = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=640x360:r=25:d=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-c:v",
            "libx264",
            "-threads",
            "1",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(fixture.0.join("input.mp4"))
        .status()
        .unwrap();
    assert!(result.success());
    fs::write(fixture.0.join("source.srt"), "7\n00:00:00,123 --> 00:00:01,987\nWe invested $20 trillion.\n\n12\n00:00:02,000 --> 00:00:02,980\nHello world.\n\n").unwrap();
    let mock = fixture.0.join("mock-codex");
    fs::write(&mock, r#"#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
payload = json.loads(sys.stdin.read().split('Input JSON:\n', 1)[1])
root = pathlib.Path(os.environ['WORKFLOW_FIXTURE'])
with (root / 'requests').open('a') as f:
    f.write(payload['task'] + ':' + payload['stage'] + '\n')
if payload['task'] == 'translation' and (root / 'fail-translation').exists():
    sys.exit(7)
cues = []
for cue in payload['cues']:
    text = cue['source_text']
    if payload['task'] == 'translation':
        text = '译文：' + text
    cues.append({'cue_id': cue['cue_id'], 'text': text})
pathlib.Path(args[args.index('--output-last-message')+1]).write_text(json.dumps({'cues': list(reversed(cues)), 'issues': []}), encoding='utf-8')
"#).unwrap();
    fs::set_permissions(&mock, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(fixture.0.join("config.toml"), format!(
        "[runtime]\nhome = {:?}\n[agent]\nskill_path = {:?}\n[release]\n[transcribe.model]\nid = \"base\"\n[transcribe.cleanup]\ncommand = {:?}\nmodel = \"gpt-5.6-terra\"\n[translate]\ncommand = {:?}\nmodel = \"gpt-5.6-luna\"\n",
        fixture.0.join("runtime"), fixture.0.join("SKILL.md"), mock, mock,
    )).unwrap();
    fixture
}

fn run(fixture: &Fixture, args: &[&str]) -> Output {
    fixture
        .command()
        .env("WORKFLOW_FIXTURE", &fixture.0)
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn start(fixture: &Fixture, extra: &[&str]) -> Output {
    let mut args = vec![
        "workflow",
        "start",
        "input.mp4",
        "--project",
        "job",
        "--subs",
        "source.srt",
        "--to",
        "zh-hans",
        "--config-path",
        "config.toml",
    ];
    args.extend_from_slice(extra);
    run(fixture, &args)
}

fn status(fixture: &Fixture) -> Value {
    serde_json::from_slice(&success(run(fixture, &["workflow", "status", "job", "--json"])).stdout)
        .unwrap()
}

fn selected(fixture: &Fixture) -> PathBuf {
    PathBuf::from(status(fixture)["selected_subtitles"].as_str().unwrap())
}

fn accept(fixture: &Fixture) {
    success(run(
        fixture,
        &[
            "workflow",
            "accept",
            "job",
            "--note",
            "Reviewed fixture captions and acknowledged readability issues.",
        ],
    ));
}

fn request_count(fixture: &Fixture) -> usize {
    fs::read_to_string(fixture.0.join("requests"))
        .unwrap()
        .lines()
        .count()
}

#[test]
fn workflow_requires_current_acceptance_then_renders_and_verifies_without_clobbering() {
    let fixture = fixture();
    let original = fs::read(fixture.0.join("input.mp4")).unwrap();
    success(start(&fixture, &[]));
    assert_eq!(status(&fixture)["state"], "review_required");
    assert_eq!(request_count(&fixture), 4);
    let target = selected(&fixture);
    assert!(
        fs::read_to_string(&target)
            .unwrap()
            .contains("7\n00:00:00,123 --> 00:00:01,987")
    );
    assert!(
        !run(&fixture, &["workflow", "render", "job"])
            .status
            .success()
    );
    assert!(!fixture.0.join("job/final.mp4").exists());
    accept(&fixture);
    let revised = fs::read_to_string(&target)
        .unwrap()
        .replace("Hello world.", "Hello, world!");
    fs::write(&target, revised).unwrap();
    assert_eq!(status(&fixture)["state"], "review_required");
    assert!(
        !run(&fixture, &["workflow", "render", "job"])
            .status
            .success()
    );
    accept(&fixture);
    success(run(
        &fixture,
        &["workflow", "render", "job", "--size", "32"],
    ));
    assert_eq!(status(&fixture)["state"], "rendered");
    let report: Value = serde_json::from_slice(
        &fs::read(fixture.0.join("job/render-0001/verification.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["verification"]["full_decode_passed"], true);
    assert_eq!(
        report["verification"]["audio"][0]["packet_data_sha256_identical"],
        true
    );
    assert_eq!(report["review"]["manually_edited_selected"], true);
    let output = fs::read(fixture.0.join("job/final.mp4")).unwrap();
    assert!(
        !run(&fixture, &["workflow", "render", "job"])
            .status
            .success()
    );
    assert_eq!(fs::read(fixture.0.join("job/final.mp4")).unwrap(), output);
    assert_eq!(fs::read(fixture.0.join("input.mp4")).unwrap(), original);
}

#[test]
fn resume_preserves_completed_source_and_old_translations_after_manual_source_edits() {
    let fixture = fixture();
    fs::write(fixture.0.join("fail-translation"), "fail once").unwrap();
    assert!(!start(&fixture, &[]).status.success());
    assert_eq!(status(&fixture)["state"], "translation_pending");
    assert_eq!(request_count(&fixture), 3);
    let source = fs::read(fixture.0.join("job/source-0001/captions.srt")).unwrap();
    fs::remove_file(fixture.0.join("fail-translation")).unwrap();
    success(run(&fixture, &["workflow", "resume", "job"]));
    assert_eq!(request_count(&fixture), 5);
    assert_eq!(
        fs::read(fixture.0.join("job/source-0001/captions.srt")).unwrap(),
        source
    );
    let previous_target = selected(&fixture);
    let previous_text = fs::read_to_string(&previous_target)
        .unwrap()
        .replace("Hello world.", "Manually checked.");
    fs::write(&previous_target, &previous_text).unwrap();
    let source_path = PathBuf::from(status(&fixture)["source_subtitles"].as_str().unwrap());
    let source_text = fs::read_to_string(&source_path)
        .unwrap()
        .replace("Hello world.", "Hello everyone.");
    fs::write(source_path, source_text).unwrap();
    assert_eq!(status(&fixture)["state"], "translation_stale");
    assert!(
        !run(
            &fixture,
            &["workflow", "accept", "job", "--note", "Reviewed"]
        )
        .status
        .success()
    );
    success(run(&fixture, &["workflow", "resume", "job"]));
    assert_eq!(request_count(&fixture), 7);
    assert!(selected(&fixture).ends_with(Path::new("target-0002/captions.srt")));
    assert_eq!(fs::read_to_string(&previous_target).unwrap(), previous_text);
    success(run(
        &fixture,
        &["workflow", "resume", "job", "--retranslate"],
    ));
    assert_eq!(request_count(&fixture), 9);
    assert!(selected(&fixture).ends_with(Path::new("target-0003/captions.srt")));
    success(run(
        &fixture,
        &["workflow", "resume", "job", "--review-source"],
    ));
    assert_eq!(request_count(&fixture), 11);
    assert!(
        PathBuf::from(status(&fixture)["source_subtitles"].as_str().unwrap())
            .ends_with(Path::new("source-0002/captions.srt"))
    );
    assert!(selected(&fixture).ends_with(Path::new("target-0003/captions.srt")));
    assert_eq!(fs::read_to_string(previous_target).unwrap(), previous_text);
    success(run(&fixture, &["workflow", "resume", "job"]));
    assert_eq!(request_count(&fixture), 11);
}

#[test]
fn watermark_snapshot_survives_preset_removal_and_edits_invalidate_acceptance() {
    let fixture = fixture();
    success(run(
        &fixture,
        &[
            "watermark-preset",
            "save",
            "lilac",
            "--text",
            "LILAC",
            "--position",
            "top-right",
            "--size",
            "48",
            "--config-path",
            "config.toml",
        ],
    ));
    success(start(&fixture, &["--watermark", "lilac"]));
    success(run(
        &fixture,
        &[
            "watermark-preset",
            "remove",
            "lilac",
            "--config-path",
            "config.toml",
        ],
    ));
    accept(&fixture);
    let manifest = fixture.0.join("job/watermark/preset.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["style"]["size"] = 40.into();
    value["style"]["colour"] = " white ".into();
    value["source"]["text"] = " LILAC ".into();
    fs::write(&manifest, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert_eq!(status(&fixture)["state"], "review_required");
    assert!(
        !run(&fixture, &["workflow", "render", "job"])
            .status
            .success()
    );
    accept(&fixture);
    success(run(&fixture, &["workflow", "render", "job"]));
    assert_eq!(status(&fixture)["state"], "rendered");
}

#[test]
fn altered_media_and_out_of_sync_translation_timing_cannot_be_accepted() {
    let fixture = fixture();
    success(start(&fixture, &[]));
    let target = selected(&fixture);
    let original = fs::read_to_string(&target).unwrap();
    fs::write(&target, original.replace("00:00:00,123", "00:00:00,124")).unwrap();
    assert!(
        !run(
            &fixture,
            &["workflow", "accept", "job", "--note", "Reviewed"]
        )
        .status
        .success()
    );
    fs::write(target, original).unwrap();
    fs::write(fixture.0.join("input.mp4"), "different media").unwrap();
    assert!(
        !run(&fixture, &["workflow", "status", "job", "--json"])
            .status
            .success()
    );
    assert!(
        !run(
            &fixture,
            &["workflow", "accept", "job", "--note", "Reviewed"]
        )
        .status
        .success()
    );
}

#[test]
fn invalid_glossary_fails_before_creating_a_project_or_calling_agents() {
    let fixture = fixture();
    fs::write(fixture.0.join("context.txt"), "a".repeat(32_001)).unwrap();
    let result = start(&fixture, &["--context-file", "context.txt"]);
    assert!(!result.status.success());
    assert!(!fixture.0.join("job").exists());
    assert!(!fixture.0.join("requests").exists());
}
