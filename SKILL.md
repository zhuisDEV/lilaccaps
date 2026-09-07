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
- OpenClaw skill path: `~/.openclaw/skills/lilaccaps/SKILL.md`
- Runtime home: `~/.lilac/lilaccaps`
- Models: `~/.lilac/lilaccaps/models`
- Temp files: `~/.lilac/lilaccaps/tmp`

`lilaccaps.toml` should keep `agent.skill_path` pointed at the OpenClaw skill path.

## Health

Run this first when unsure:

```bash
lilaccaps --version
lilaccaps status
lilaccaps doctor
```

`lilaccaps --version` always reports the installed version. If a newer stable release is available,
it also reports `new_version` and recommends `lilaccaps update`; an unavailable release check does
not make the command fail.

Healthy status should include `healthy = true`, `config_valid = true`, `model_ready = true`, and `missing = none`.
Dependency entries include the resolved executable path, detected version, and any startup error. Use `lilaccaps doctor --fix`
on macOS to install or repair unhealthy Homebrew packages.

## Install

Recommended: install globally from GitHub with the remote installer:

```bash
curl -fsSL https://raw.githubusercontent.com/zhuisDEV/lilaccaps/main/install.sh | sh -s -- --fix
```

Omit `--fix` only when `ffmpeg`, `ffprobe`, `cmake`, and ImageMagick are managed separately.

If you do not want to pipe a remote script into `sh`, use Cargo directly:

```bash
cargo install --git https://github.com/zhuisDEV/lilaccaps.git --locked --force lilaccaps
"${CARGO_HOME:-$HOME/.cargo}/bin/lilaccaps" install
```

Update lilaccaps and its managed Homebrew dependencies:

```bash
lilaccaps update
```

Use `lilaccaps update --skip-dependencies` only when system dependencies are managed separately.

## Basic CLI

Transcribe media to subtitles:

```bash
lilaccaps transcribe ./input.mp4
lilaccaps transcribe ./input.mp4 --lang zh
lilaccaps transcribe ./input.mp4 --lang zh --engine faster-whisper --model large-v3-turbo
lilaccaps transcribe ./input.mp4 --lang zh --engine faster-whisper --cleanup
```

`whisper-rs` is the default, simplest local engine. It uses adaptive speech-aware segmentation,
bridges short pauses, pads speech, omits long silence, bounds continuous speech with overlapping
windows, and deduplicates boundaries. Faster-whisper is the higher-quality opt-in engine; it uses
the pinned uv-managed helper, `large-v3-turbo`/`large-v3`, Silero VAD, and word timestamps. Both
engines use the same cue builder and final SRT validation. Set
`transcribe.segmentation.mode = "fixed"` only when fixed `whisper-rs` windows are preferred.

Translate an existing subtitle file:

```bash
lilaccaps translate ./input.srt --to en --append
lilaccaps translate ./input.srt --to zh-hans --to en --append
```

Translation uses `gpt-5.6-luna` with `medium` reasoning through a recent Codex CLI supporting
`--ignore-user-config` (verified with 0.153.4). It reuses your Codex app ChatGPT OAuth login;
check it with `codex login status`. Keep the same `CODEX_HOME` as the app. Gemini translation
is retired and no translation API key is needed.

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

Burn-in preserves explicit `.srt` line breaks. When advanced styling uses the ImageMagick
overlay renderer, long subtitle lines are wrapped inside the video frame before rendering.

## Basic Settings

Edit `~/.lilac/lilaccaps/lilaccaps.toml`.

Common settings:

```toml
[transcribe]
engine = "whisper-rs"
language = "auto"

[transcribe.segmentation]
mode = "speech"
chunk_seconds = 30
overlap_seconds = 2
min_speech_ms = 400
min_silence_ms = 350
padding_ms = 300
max_window_seconds = 30

[transcribe.cues]
min_duration_ms = 800
max_duration_ms = 6000
end_padding_ms = 150
pause_split_ms = 500
max_chars_per_line = 42
max_cjk_chars_per_line = 18
max_lines = 2

[transcribe.model]
id = "medium"

[transcribe.cleanup]
enabled = false
command = "codex"
model = "gpt-5.6-terra"
reasoning_effort = "medium"

[translate]
command = "codex"
model = "gpt-5.6-luna"
reasoning_effort = "medium"
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

`transcribe.cleanup.reasoning_effort` defaults to `"medium"` and overrides the global Codex
reasoning setting. Choose an effort supported by the cleanup model. Cleanup remains disabled
by default and uses Terra unless its model is overridden.

`translate.model` also accepts `openai/gpt-5.6-luna`. `translate.reasoning_effort` accepts
`low`, `medium`, `high`, `xhigh`, or `max`; it is passed explicitly to Codex. Existing `gemini-*`
model settings migrate to Luna on load, and install or update persists the migration.

Transcription builds cues from Whisper token timestamps by default, preferring punctuation and
pauses and applying CJK-aware character limits. `cue_timing` reports `word`, `mixed`, or
`segment-fallback`; the fallback is automatic when timed-token text cannot be reconstructed safely.

Use `--engine faster-whisper --model large-v3-turbo` when subtitle recognition quality matters more
than the simplest installation; `uv` is required and the model downloads on first use. `--cleanup`
adds an optional Codex text-only correction pass. It preserves cue count/order/timestamps, rejects
large rewrites, and fails without publishing if validation fails. Cleanup sends subtitle text to the
configured Codex provider, so leave it disabled for private or fully local workflows.

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
6. Never reuse an input path as an output path; lilaccaps rejects symlink and hard-link aliases too.
