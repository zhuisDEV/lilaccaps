# lilaccaps

[![CI](https://github.com/zhuisDEV/lilaccaps/actions/workflows/ci.yml/badge.svg)](https://github.com/zhuisDEV/lilaccaps/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)

`lilaccaps` is a clean Rust CLI for two separate workflows:

1. transcribe local video or audio into subtitle files
2. translate an existing subtitle file into one or more target languages
3. burn an existing subtitle file into a video

The command model stays explicit:

- `lilaccaps doctor` inspects local prerequisites and setup health
- `lilaccaps doctor --fix` installs missing macOS packages through Homebrew when possible
- `lilaccaps transcribe` generates subtitle artifacts such as `.srt`
- `lilaccaps translate` adds one or more translated lines to subtitle cues
- `lilaccaps burnin` is render-only and never hides transcription inside the command

## Project Cards

| Transcribe | Translate | Burn-in | Lifecycle |
| --- | --- | --- | --- |
| Extract audio with `ffmpeg`, transcribe with Whisper, write `.srt` | Render an existing subtitle file into video | Install, status, update, and uninstall support |
| Primary flow stays subtitle-first | Append multilingual lines to existing cues via Gemini | Primary renderer uses `ffmpeg` subtitle filters when available | Configured with `lilaccaps.toml` and `LILACCAPS_HOME` |
| Output path is explicit | Cue timing and indexes stay unchanged | Fallback renderer uses ImageMagick overlays when needed | OpenClaw skill bootstrap is supported |

## Features

- Unified CLI under `lilaccaps`
- `transcribe`, `translate`, and `burnin` kept as separate first-class workflows
- Managed Whisper model download under the runtime home
- Runtime health checks through `lilaccaps status`
- Generated OpenClaw skill bootstrap
- Clean primary and fallback separation for rendering
- `.env` support for translation API credentials

## Requirements

- Rust toolchain with `cargo`
- `ffmpeg`
- `ffprobe`
- `cmake`
- ImageMagick `magick`

For native `burnin` rendering, prefer an `ffmpeg` build that includes the `subtitles` or
`ass` filters through `libass`. On macOS with Homebrew, `ffmpeg-full` is the reliable option.
Without those filters, `lilaccaps` falls back to ImageMagick overlay rendering.

On macOS with Homebrew:

```bash
brew install ffmpeg-full cmake imagemagick
```

## Install

Build locally and install with cargo:

```bash
brew install ffmpeg-full cmake imagemagick
cargo install --path .
lilaccaps install
```

If `cmake` is missing, `cargo install` fails while compiling `whisper-rs-sys` before the
`lilaccaps` binary exists. Install prerequisites first, then rerun `cargo install`.

`doctor --fix` installs the minimum mapped packages through Homebrew after the binary exists,
but it does not currently upgrade a plain `ffmpeg` install to `ffmpeg-full`. If you want
native subtitle burn-in instead of the ImageMagick fallback, install an `ffmpeg` build with
`libass` support yourself.

After the binary exists, you can inspect or repair prerequisites with:

```bash
lilaccaps doctor
lilaccaps doctor --fix
lilaccaps install --fix
```

`--fix` currently supports macOS through Homebrew and installs only the missing packages
that `lilaccaps` maps explicitly.

After installation, `lilaccaps status` reports:

- core runtime readiness (`healthy`, `missing`)
- build/update readiness (`cargo_available`, `cmake_available`, `build_ready`)
- fallback renderer readiness (`magick_available`, `fallback_renderer_ready`)
- fixability (`brew_packages`, `can_fix_with_brew`)

This initializes:

- `~/.lilac/lilaccaps/lilaccaps.toml`
- runtime home, defaulting to `~/.lilac/lilaccaps`
- Whisper model assets under the runtime home
- OpenClaw skill bootstrap files

For translation, create a local `.env` file under the runtime home, for example
`~/.lilac/lilaccaps/.env`, and set:

```bash
GEMINI_API_KEY=your_api_key_here
```

## Usage

Transcribe into subtitles:

```bash
lilaccaps transcribe ./input.mp4
lilaccaps transcribe ./input.mp4 --lang zh
```

Translate an existing `.srt` into one or more target languages:

```bash
lilaccaps translate ./input.srt --to en --append
lilaccaps translate ./input.srt --to en --to ja --append
```

That produces multilingual cue text such as:

```text
原文中文
English translation
日本語訳
```

Burn in an existing subtitle file:

```bash
lilaccaps burnin ./input.mp4 --subs ./input.srt
lilaccaps burnin ./input.mp4 --subs ./input.srt --font "PingFang SC" --size 42
```

Check environment health:

```bash
lilaccaps status
lilaccaps doctor
```

## Configuration

The default config file is `~/.lilac/lilaccaps/lilaccaps.toml`.

Important values:

- `runtime.home`
- `agent.skill_path`
- `release.github_repo`
- `transcribe.language`
- `transcribe.model.id`
- `transcribe.model.path`
- `translate.model`
- `translate.append`
- `translate.default_targets`
- `burnin.font`
- `burnin.size`
- `burnin.line_spacing`

`LILACCAPS_HOME` overrides the runtime home at runtime.

`transcribe.language` defaults to `"auto"`. Set it to a Whisper language code such as
`"en"`, `"zh"`, or `"ja"` to force transcription in a specific language. `--lang` on
`lilaccaps transcribe` overrides the config value for a single run.

When `language = "auto"`, `lilaccaps` now performs a dedicated Whisper language-detection pass
first and then transcribes with the detected language explicitly. That avoids the
`detect_language=true` detect-only path in `whisper.cpp` and reports the effective language in
the command output.

When you force a language, `lilaccaps` tries:

- requested language with beam search
- requested language with greedy decoding
- detected language with beam search, if detection succeeds and differs
- detected language with greedy decoding, if detection succeeds and differs

If all attempts still produce no subtitle text, the terminal error includes per-attempt
segment diagnostics.

Supported `transcribe.model.id` values currently include `tiny`, `base`, `small`, `medium`
and their `.en` variants. For Chinese, Japanese, and other non-English speech, use the
non-`.en` models.

`translate.model` defaults to `"gemini-3.1-flash-lite-preview"`. `translate.append`
defaults to `true`, which means translated lines are appended below the original cue text.
`translate.default_targets` can provide default `--to` languages for `lilaccaps translate`.
`translate.line_order` controls top-to-bottom line order inside each multilingual cue.

Example:

```toml
[translate]
model = "gemini-3.1-flash-lite-preview"
append = true
default_targets = ["en", "ja"]
line_order = ["source", "ja", "en"]
```

`lilaccaps translate` loads `GEMINI_API_KEY` from the environment or from
`$LILACCAPS_HOME/.env` (default `~/.lilac/lilaccaps/.env`). It keeps cue timing and
indexes unchanged and only rewrites cue text.

`burnin.font` defaults to `"auto"`, which lets the renderer choose a suitable font. The
ImageMagick fallback prefers a CJK-capable font for CJK subtitles and a lighter Latin font
otherwise. Set `burnin.font` to a system font name such as `"PingFang SC"` or `"Arial"` to
force a specific font for burn-in, or pass `--font` on the CLI for a single run.

`burnin.size` defaults to `0`, which means auto-size from the video height. Set a positive
number in `lilaccaps.toml` or pass `--size` on the CLI to force a point size. CLI values
override TOML values for a single run.

`burnin.line_spacing` defaults to `0`, which means auto spacing. Set a positive number in
`lilaccaps.toml` to control the vertical gap between lines in multiline burn-in subtitles.
This setting is applied by the ImageMagick overlay renderer; when you set it explicitly,
`lilaccaps` prefers that renderer so the spacing value is honored.

For per-language burn-in styling, define keyed styles under `burnin.styles` using the same
labels that appear in `translate.line_order`, such as `source`, `en`, or `ja`. Set
`font = "auto"` or omit `font` to keep renderer font selection per line.

Example:

```toml
[translate]
append = true
default_targets = ["en", "ja"]
line_order = ["source", "ja", "en"]

[burnin]
font = "auto"
size = 0
line_spacing = 1

[burnin.styles.source]
font = "auto"
size = 42

[burnin.styles.ja]
font = "auto"
size = 38

[burnin.styles.en]
font = "auto"
size = 34
```

With that config, the first line in each cue uses the `source` style, the second uses `ja`,
and the third uses `en`.

## Repository Layout

```text
src/
  commands/
  pipelines/
  media.rs
  model.rs
  render.rs
  subtitles.rs
```

Supporting planning docs:

- [lilaccaps-plan.md](./lilaccaps-plan.md)
- [handoff.md](./handoff.md)
- [video-subtitle-pipeline-proposal.md](./video-subtitle-pipeline-proposal.md)

## Status

This project is usable now for:

- local transcription to `.srt`
- burn-in rendering from existing `.srt`
- installation and environment health reporting

The current implementation favors clean boundaries over broad convenience wrappers.

## License

MIT. See [LICENSE](./LICENSE).
