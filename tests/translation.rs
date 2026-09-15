#![cfg(unix)]
mod common;
use common::Fixture;
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn setup(fixture: &Fixture, model: &str, response: &str, exit_code: i32) {
    let command = fixture.0.join("mock-codex");
    fs::write(
        &command,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$@" > "$TRANSLATE_ARGS"
if [ ! -f "$TRANSLATE_ARGS.generate" ]; then printf '%s\n' "$@" > "$TRANSLATE_ARGS.generate"; fi
output=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = '--output-last-message' ]; then shift; output="$1"; fi
  shift
done
cat > "$TRANSLATE_PROMPT"
printf '%s' '{response}' > "$output"
exit {exit_code}
"#
        ),
    )
    .unwrap();
    fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(fixture.0.join("config.toml"), format!(
        "[runtime]\nhome = {:?}\n[agent]\nskill_path = {:?}\n[release]\n[transcribe.model]\nid = \"base\"\n[translate]\ncommand = {:?}\nmodel = {:?}\n",
        fixture.0.join("runtime"), fixture.0.join("SKILL.md"), command, model
    )).unwrap();
    fs::write(
        fixture.0.join("input.srt"),
        "1\n00:00:00,123 --> 00:00:01,987\nHello!\n\n",
    )
    .unwrap();
}

fn run(fixture: &Fixture) -> std::process::Output {
    fixture
        .command()
        .env("TRANSLATE_ARGS", fixture.0.join("args"))
        .env("TRANSLATE_PROMPT", fixture.0.join("prompt"))
        .args([
            "translate",
            "input.srt",
            "--config-path",
            "config.toml",
            "--to",
            "zh-hans",
            "--output",
            "output.srt",
        ])
        .output()
        .unwrap()
}

#[test]
fn codex_translation_migrates_gemini_and_preserves_milliseconds() {
    for model in [
        "gemini-3.1-flash-lite",
        "gemini-3.1-flash-lite-preview",
        "openai/gpt-5.6-luna",
    ] {
        let fixture = Fixture::new();
        setup(
            &fixture,
            model,
            r#"{"cues":[{"cue_id":1,"text":"你好！"}],"issues":[]}"#,
            0,
        );
        let output = run(&fixture);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let args = fs::read_to_string(fixture.0.join("args")).unwrap();
        for required in [
            "--model\ngpt-5.6-terra\n",
            "model_reasoning_effort=\"medium\"",
            "--ignore-user-config",
            "forced_login_method=\"chatgpt\"",
            "--sandbox\nread-only\n",
            "--ephemeral",
        ] {
            assert!(args.contains(required), "missing {required}: {args}");
        }
        assert!(
            fs::read_to_string(fixture.0.join("args.generate"))
                .unwrap()
                .contains("--model\ngpt-5.6-luna\n")
        );
        let report: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(fixture.0.join("output.srt.review.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(report["targets"][0]["review"]["generation_passes"], 1);
        assert_eq!(report["targets"][0]["review"]["verification_passes"], 1);
        assert_eq!(report["targets"][0]["review"]["review_mode"], "text_only");
        assert_eq!(report["targets"][0]["review"]["model"], "gpt-5.6-luna");
        assert_eq!(
            report["targets"][0]["review"]["verification_model"],
            "gpt-5.6-terra"
        );
        let srt = fs::read_to_string(fixture.0.join("output.srt")).unwrap();
        assert!(srt.contains("00:00:00,123 --> 00:00:01,987\nHello!\n你好！"));
        assert!(
            fs::read_to_string(fixture.0.join("prompt"))
                .unwrap()
                .contains("Hello!")
        );
        assert_eq!(
            fs::read_dir(fixture.0.join("runtime/tmp")).unwrap().count(),
            0
        );
    }
}

#[test]
fn bad_translation_never_overwrites_existing_output() {
    for (response, exit_code) in [
        (r#"{"cues":[],"issues":[]}"#, 0),
        (r#"{"cues":[{"cue_id":1,"text":" "}],"issues":[]}"#, 0),
        ("malformed", 0),
        (r#"{"cues":[{"cue_id":1,"text":"你好"}],"issues":[]}"#, 1),
    ] {
        let fixture = Fixture::new();
        setup(&fixture, "gpt-5.6-luna", response, exit_code);
        fs::write(fixture.0.join("output.srt"), "existing output").unwrap();
        fs::write(fixture.0.join("output.srt.review.json"), "existing review").unwrap();
        assert!(!run(&fixture).status.success());
        assert_eq!(
            fs::read_to_string(fixture.0.join("output.srt")).unwrap(),
            "existing output"
        );
        assert_eq!(
            fs::read_to_string(fixture.0.join("output.srt.review.json")).unwrap(),
            "existing review"
        );
    }
}

#[test]
fn explicit_effort_is_forwarded_and_invalid_effort_fails_before_launch() {
    for effort in ["high", "ultra"] {
        let fixture = Fixture::new();
        setup(
            &fixture,
            "gpt-5.6-luna",
            r#"{"cues":[{"cue_id":1,"text":"你好"}],"issues":[]}"#,
            0,
        );
        let config_path = fixture.0.join("config.toml");
        let mut config = fs::read_to_string(&config_path).unwrap();
        config.push_str(&format!("reasoning_effort = {effort:?}\n"));
        fs::write(config_path, config).unwrap();
        let output = run(&fixture);
        if effort == "high" {
            assert!(output.status.success());
            assert!(
                fs::read_to_string(fixture.0.join("args.generate"))
                    .unwrap()
                    .contains("model_reasoning_effort=\"high\"")
            );
        } else {
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("translate.reasoning_effort"));
            assert!(!fixture.0.join("args").exists());
        }
    }
}

#[test]
fn translation_verifier_model_and_effort_are_independently_configurable_and_validated() {
    for (review_model, review_effort, valid) in [
        ("gpt-6-astra", "high", true),
        ("gemini-other", "medium", false),
        ("gpt-5.6-terra", "ultra", false),
    ] {
        let fixture = Fixture::new();
        setup(
            &fixture,
            "gpt-5.6-luna",
            r#"{"cues":[{"cue_id":1,"text":"你好！"}],"issues":[]}"#,
            0,
        );
        let path = fixture.0.join("config.toml");
        let config = fs::read_to_string(&path).unwrap();
        fs::write(path, format!("{config}review_model = {review_model:?}\nreview_reasoning_effort = {review_effort:?}\n")).unwrap();
        let output = run(&fixture);
        assert_eq!(
            output.status.success(),
            valid,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if valid {
            let args = fs::read_to_string(fixture.0.join("args")).unwrap();
            assert!(args.contains("--model\ngpt-6-astra\n"));
            assert!(args.contains("model_reasoning_effort=\"high\""));
            let args = fs::read_to_string(fixture.0.join("args.generate")).unwrap();
            assert!(args.contains("--model\ngpt-5.6-luna\n"));
            assert!(args.contains("model_reasoning_effort=\"medium\""));
        } else {
            assert!(String::from_utf8_lossy(&output.stderr).contains("translate.review_"));
            assert!(!fixture.0.join("args").exists());
        }
    }
}

#[test]
fn later_batch_failure_never_publishes_a_partial_translation_or_report() {
    let fixture = Fixture::new();
    setup(&fixture, "gpt-5.6-luna", r#"{"cues":[],"issues":[]}"#, 0);
    fs::write(
        fixture.0.join("mock-codex"),
        r#"#!/usr/bin/env python3
import json, pathlib, sys
args = sys.argv[1:]
data = json.loads(sys.stdin.read().split("Input JSON:\n", 1)[1])
output = pathlib.Path(args[args.index("--output-last-message") + 1])
result = {"cues": [{"cue_id": cue["cue_id"], "text": "你好"} for cue in data["cues"]], "issues": []}
if data["cues"][0]["cue_id"] == 25:
    result["cues"] = []
output.write_text(json.dumps(result))
"#,
    )
    .unwrap();
    let input = (1..=25)
        .map(|index| {
            format!(
                "{index}\n00:00:{:02},123 --> 00:00:{:02},987\nHello!\n\n",
                (index - 1) * 2,
                (index - 1) * 2 + 1
            )
        })
        .collect::<String>();
    fs::write(fixture.0.join("input.srt"), &input).unwrap();
    fs::write(fixture.0.join("output.srt"), "existing captions").unwrap();
    fs::write(fixture.0.join("output.srt.review.json"), "existing review").unwrap();
    let result = run(&fixture);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cue 25"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("output.srt")).unwrap(),
        "existing captions"
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("output.srt.review.json")).unwrap(),
        "existing review"
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("input.srt")).unwrap(),
        input
    );
}
