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
        setup(&fixture, model, r#"{"translations":["你好！"]}"#, 0);
        let output = run(&fixture);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let args = fs::read_to_string(fixture.0.join("args")).unwrap();
        for required in [
            "--model\ngpt-5.6-luna\n",
            "model_reasoning_effort=\"medium\"",
            "--ignore-user-config",
            "forced_login_method=\"chatgpt\"",
            "--sandbox\nread-only\n",
            "--ephemeral",
        ] {
            assert!(args.contains(required), "missing {required}: {args}");
        }
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
        (r#"{"translations":[]}"#, 0),
        (r#"{"translations":[" "]}"#, 0),
        ("malformed", 0),
        (r#"{"translations":["你好"]}"#, 1),
    ] {
        let fixture = Fixture::new();
        setup(&fixture, "gpt-5.6-luna", response, exit_code);
        fs::write(fixture.0.join("output.srt"), "existing output").unwrap();
        assert!(!run(&fixture).status.success());
        assert_eq!(
            fs::read_to_string(fixture.0.join("output.srt")).unwrap(),
            "existing output"
        );
    }
}

#[test]
fn explicit_effort_is_forwarded_and_invalid_effort_fails_before_launch() {
    for effort in ["high", "ultra"] {
        let fixture = Fixture::new();
        setup(&fixture, "gpt-5.6-luna", r#"{"translations":["你好"]}"#, 0);
        let config_path = fixture.0.join("config.toml");
        let mut config = fs::read_to_string(&config_path).unwrap();
        config.push_str(&format!("reasoning_effort = {effort:?}\n"));
        fs::write(config_path, config).unwrap();
        let output = run(&fixture);
        if effort == "high" {
            assert!(output.status.success());
            assert!(
                fs::read_to_string(fixture.0.join("args"))
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
