mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use common::Fixture;

fn success(command: &mut Command) -> Output {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn video(fixture: &Fixture) {
    success(
        Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=gray:s=640x360:r=25:d=0.4",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(fixture.0.join("input.mp4")),
    );
}

fn digest(path: &Path) -> Vec<u8> {
    success(
        Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(path)
            .args(["-frames:v", "1", "-f", "md5", "-"]),
    )
    .stdout
}

#[test]
fn named_text_preset_preserves_all_style_flags_in_the_render() {
    let fixture = Fixture::new();
    video(&fixture);
    let options = [
        "--text",
        "LILAC",
        "--position",
        "top-right",
        "--opacity",
        "0.7",
        "--size",
        "48",
        "--margin",
        "40",
        "--colour",
        "#ffd54f",
        "--outline-colour",
        "black",
        "--outline-width",
        "2",
    ];
    success(
        fixture
            .command()
            .args(["watermark-preset", "save", "lilac"])
            .args(options),
    );
    let show = success(
        fixture
            .command()
            .args(["watermark-preset", "show", "lilac", "--json"]),
    );
    let value: serde_json::Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(value["style"]["position"], "top-right");
    assert_eq!(value["style"]["size"], 48);
    assert_eq!(value["style"]["outline_width"], 2);

    let saved = fixture.0.join("saved.mp4");
    success(
        fixture
            .command()
            .arg("watermark")
            .arg(fixture.0.join("input.mp4"))
            .args(["--preset", "lilac", "--output"])
            .arg(&saved),
    );
    let inline = fixture.0.join("inline.mp4");
    success(
        fixture
            .command()
            .arg("watermark")
            .arg(fixture.0.join("input.mp4"))
            .args(options)
            .arg("--output")
            .arg(&inline),
    );
    assert_eq!(digest(&saved), digest(&inline));
    assert_ne!(digest(&saved), digest(&fixture.0.join("input.mp4")));

    for conflict in [
        "--text",
        "--image",
        "--position",
        "--opacity",
        "--size",
        "--margin",
        "--colour",
        "--font",
        "--outline-colour",
        "--outline-width",
    ] {
        let value = match conflict {
            "--position" => "top-left",
            "--opacity" => "0.2",
            "--size" | "--margin" | "--outline-width" => "2",
            _ => "value",
        };
        let result = fixture
            .command()
            .arg("watermark")
            .arg(fixture.0.join("input.mp4"))
            .args(["--preset", "lilac", conflict, value])
            .output()
            .unwrap();
        assert!(
            !result.status.success(),
            "accepted preset style override {conflict}"
        );
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("cannot be used with"),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[test]
fn image_preset_renders_after_its_source_is_removed_and_can_be_deleted() {
    let fixture = Fixture::new();
    video(&fixture);
    let logo = fixture.0.join("logo.ppm");
    let mut bytes = b"P6\n2 2\n255\n".to_vec();
    bytes.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
    fs::write(&logo, bytes).unwrap();
    success(
        fixture
            .command()
            .args(["watermark-preset", "save", "logo", "--image"])
            .arg(&logo)
            .args(["--size", "64", "--opacity", "1"]),
    );
    fs::remove_file(logo).unwrap();
    let rendered = fixture.0.join("saved.mp4");
    success(
        fixture
            .command()
            .arg("watermark")
            .arg(fixture.0.join("input.mp4"))
            .args(["--preset", "logo", "--output"])
            .arg(&rendered),
    );
    assert_ne!(digest(&rendered), digest(&fixture.0.join("input.mp4")));
    let before = success(
        fixture
            .command()
            .args(["watermark-preset", "list", "--json"]),
    );
    let presets: Vec<serde_json::Value> = serde_json::from_slice(&before.stdout).unwrap();
    assert_eq!(presets.len(), 1);
    success(
        fixture
            .command()
            .args(["watermark-preset", "remove", "logo"]),
    );
    let after = success(
        fixture
            .command()
            .args(["watermark-preset", "list", "--json"]),
    );
    assert_eq!(
        serde_json::from_slice::<Vec<serde_json::Value>>(&after.stdout)
            .unwrap()
            .len(),
        0
    );
    assert!(rendered.is_file());
}
