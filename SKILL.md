---
name: lilaccaps
description: Quick start for using the lilaccaps CLI for transcription, subtitle translation, burn-in captions, watermarks, health checks, and basic config. Use when the user wants the basic CLI commands, basic settings, or where to find advanced lilaccaps settings and full manual docs.
---

# lilaccaps Quick Start

Use `lilaccaps` for local subtitle and video workflows. Keep each workflow explicit:

- `transcribe`: media to `.srt`
- `translate`: `.srt` to translated `.srt`
- `burnin`: video plus `.srt` to captioned video
- `watermark`: video plus text or image to watermarked video
- `status` and `doctor`: health checks

Do not hide transcription, translation, and burn-in inside one command unless the user explicitly asks for an end-to-end wrapper.

## Files

- Full manual: `README.md`
- Config: `~/.lilac/lilaccaps/lilaccaps.toml`
- Runtime quick-start copy: `~/.lilac/lilaccaps/SKILL.md`
- OpenClaw skill path: `~/.openclaw/skills/lilaccaps/SKILL.md`
- Runtime home: `~/.lilac/lilaccaps`
- Models: `~/.lilac/lilaccaps/models`
- Temp files: `~/.lilac/lilaccaps/tmp`
- Optional env file: `~/.lilac/lilaccaps/.env`

`lilaccaps.toml` should keep `agent.skill_path` pointed at the OpenClaw skill path.

## Health

Run this first when unsure:

```bash
lilaccaps status
lilaccaps doctor
```

Healthy status should include `healthy = true`, `config_valid = true`, `model_ready = true`, and `missing = none`.

## Install

Recommended: install globally from GitHub with the remote installer:

```bash
curl -fsSL https://raw.githubusercontent.com/zhuisDEV/lilaccaps/main/install.sh | sh
```

If you do not want to pipe a remote script into `sh`, use Cargo directly:

```bash
cargo install --git https://github.com/zhuisDEV/lilaccaps.git --locked --force lilaccaps
lilaccaps install
```

## Basic CLI

Transcribe media to subtitles:

```bash
lilaccaps transcribe ./input.mp4
lilaccaps transcribe ./input.mp4 --lang zh
```

Translate an existing subtitle file:

```bash
lilaccaps translate ./input.srt --to en --append
lilaccaps translate ./input.srt --to zh-hans --to en --append
```

Burn subtitles into a video:

```bash
lilaccaps burnin ./input.mp4 --subs ./input.srt
lilaccaps burnin ./input.mp4 --subs ./input.srt --outline-colour black --outline-width 3
```

Add a watermark:

```bash
lilaccaps watermark ./input.mp4 --text "lilac"
lilaccaps watermark ./input.mp4 --text "Lilac Captions" --font Verdana --colour "#E19CFF" --outline-width 2
lilaccaps watermark ./input.mp4 --image ./logo.png --opacity 0.45 --size 180
```

Text watermarks use ffmpeg `drawtext` when available. If `drawtext` is missing or fails, lilaccaps
renders the text to PNG with ImageMagick and applies it through image overlay. SVG image watermarks
are converted to PNG before overlaying.

Get CLI details:

```bash
lilaccaps --help
lilaccaps transcribe --help
lilaccaps translate --help
lilaccaps burnin --help
lilaccaps watermark --help
```

## Basic Settings

Edit `~/.lilac/lilaccaps/lilaccaps.toml`.

Common settings:

```toml
[transcribe]
language = "auto"

[transcribe.model]
id = "medium"

[translate]
model = "gemini-3.1-flash-lite-preview"
append = true
default_targets = ["zh-hans", "en"]
line_order = ["source", "zh-hans", "en"]

[burnin]
advanced_styling = false
font = "auto"
colour = "auto"
size = 0
line_spacing = 1

[burnin.outline]
enabled = true
colour = "black"
width = 2
```

## Advanced Settings and CLI

Use `README.md` for advanced behavior, renderer details, per-line burn-in styles, model choices, update flow, troubleshooting, and all config keys.

Use command help for exact current CLI flags:

```bash
lilaccaps <command> --help
```

## Operating Rules

When using `lilaccaps`:

1. Confirm the input file exists.
2. Run `status` or `doctor` if environment readiness is uncertain.
3. Use the explicit command for the task.
4. Prefer explicit `--output` paths for important outputs.
5. Report generated file paths and burn-in renderer choice.
