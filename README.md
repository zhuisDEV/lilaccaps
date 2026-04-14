# lilaccaps

[![CI](https://github.com/zhuisDEV/lilaccaps/actions/workflows/ci.yml/badge.svg)](https://github.com/zhuisDEV/lilaccaps/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)

`lilaccaps` is a clean Rust CLI for two separate workflows:

1. transcribe local video or audio into subtitle files
2. burn an existing subtitle file into a video

The command model stays explicit:

- `lilaccaps doctor` inspects local prerequisites and setup health
- `lilaccaps doctor --fix` installs missing macOS packages through Homebrew when possible
- `lilaccaps transcribe` generates subtitle artifacts such as `.srt`
- `lilaccaps burnin` is render-only and never hides transcription inside the command

## Project Cards

| Transcribe | Burn-in | Lifecycle |
| --- | --- | --- |
| Extract audio with `ffmpeg`, transcribe with Whisper, write `.srt` | Render an existing subtitle file into video | Install, status, update, and uninstall support |
| Primary flow stays subtitle-first | Primary renderer uses `ffmpeg` subtitle filters when available | Configured with `lilaccaps.toml` and `LILACCAPS_HOME` |
| Output path is explicit | Fallback renderer uses ImageMagick overlays when needed | OpenClaw skill bootstrap is supported |

## Features

- Unified CLI under `lilaccaps`
- `transcribe` and `burnin` kept as separate first-class workflows
- Managed Whisper model download under the runtime home
- Runtime health checks through `lilaccaps status`
- Generated OpenClaw skill bootstrap
- Clean primary and fallback separation for rendering

## Requirements

- Rust toolchain with `cargo`
- `ffmpeg`
- `ffprobe`
- `cmake`
- ImageMagick `magick`

On macOS with Homebrew:

```bash
brew install ffmpeg cmake imagemagick
```

## Install

Build locally and install with cargo:

```bash
brew install ffmpeg cmake imagemagick
cargo install --path .
lilaccaps install
```

If `cmake` is missing, `cargo install` fails while compiling `whisper-rs-sys` before the
`lilaccaps` binary exists. Install prerequisites first, then rerun `cargo install`.

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

## Usage

Transcribe into subtitles:

```bash
lilaccaps transcribe ./input.mp4
```

Burn in an existing subtitle file:

```bash
lilaccaps burnin ./input.mp4 --subs ./input.srt
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
- `transcribe.model.id`
- `transcribe.model.path`

`LILACCAPS_HOME` overrides the runtime home at runtime.

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
