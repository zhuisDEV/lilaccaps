# lilaccaps Product and Engineering Plan

## Purpose

`lilaccaps` is the canonical Rust CLI for local subtitle and video-rendering workflows. Each command
owns one explicit artifact transformation; lifecycle work remains separate from media processing.

## Current Product

Implemented commands:

```text
lilaccaps doctor [--fix]
lilaccaps install [--fix]
lilaccaps update [--skip-dependencies]
lilaccaps status [--json]
lilaccaps uninstall --yes
lilaccaps transcribe <media> [--lang <code>] [--output <file.srt>]
lilaccaps translate <file.srt> --to <language> [--append <bool>]
lilaccaps burnin <video> --subs <file.srt> [--output <video>]
lilaccaps watermark <video> (--text <text> | --image <path>) [--output <video>]
```

Current capabilities:

- local Whisper transcription with automatic or explicit language selection
- Gemini subtitle translation while preserving cue indexes and timing
- primary FFmpeg/libass burn-in plus an explicit ImageMagick overlay fallback
- FFmpeg text/image watermarking plus ImageMagick conversion and text fallback
- GitHub remote installation, release discovery, self-update, health checks, and owned uninstall
- Homebrew dependency health, repair, and update support on macOS
- OpenClaw generated skill integration and bootstrap documentation

Soft-subtitle muxing (`embed`), `.vtt`, plain transcript output, speaker segmentation, and a combined
download-to-caption wrapper are not implemented commands.

## Design Rules

- Keep `transcribe`, `translate`, `burnin`, and `watermark` as separate workflows.
- Do not make `burnin` or `watermark` trigger transcription implicitly.
- Keep command handlers thin and pipeline modules responsible for the actual work.
- Report exact input/output paths and the selected renderer.
- Keep primary and fallback renderers distinguishable in code and command output.
- Reject output paths that alias any input and publish files atomically after successful work.
- Use uniquely scoped temporary assets and clean them on both success and failure.
- Treat recursive deletion, credentials, release installation, and external commands as security
  boundaries.

## Runtime Contract

- Config: `~/.lilac/lilaccaps/lilaccaps.toml`
- Runtime override: `LILACCAPS_HOME`
- Models: `$LILACCAPS_HOME/models`
- Temporary assets: `$LILACCAPS_HOME/tmp`
- Ownership marker: `$LILACCAPS_HOME/.lilaccaps-runtime`
- OpenClaw skill default: `~/.openclaw/skills/lilaccaps/SKILL.md`
- Translation credential: exported `GEMINI_API_KEY`, then `$LILACCAPS_HOME/.env`

Paths beginning with `~` are expanded after config parsing. Explicit environment values take
precedence over file-based defaults.

## Architecture

- `src/commands`: argument-to-pipeline dispatch and structured command summaries
- `src/pipelines`: one module per media workflow
- `src/media.rs`: FFmpeg/FFprobe boundaries and media probing
- `src/model.rs`: Whisper model resolution and atomic streaming download
- `src/subtitles.rs`: SRT parsing and atomic serialization
- `src/render.rs`: burn-in renderer selection and overlay implementation
- `src/watermark.rs`: text/image watermark renderer selection
- `src/runtime.rs`: dependencies, paths, temporary assets, ownership, and lifecycle safety
- `src/config.rs`, `src/release.rs`, `src/integration.rs`: config migration, release discovery, and
  OpenClaw integration

## Release Gate

Every release must complete:

```bash
cargo fmt --all --check
shellcheck install.sh
actionlint
gitleaks detect --source . --no-banner --redact
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
cargo audit
cargo build --release --locked
```

Changes to media pipelines also require disposable end-to-end FFmpeg tests and `ffprobe` validation.
Release publication happens only after the pushed commit passes GitHub Actions.

## Roadmap

Priorities are driven by concrete user workflows rather than command count:

1. Add model integrity metadata or checksums without buffering downloads in memory.
2. Batch very large translation jobs with deterministic cue-count validation per batch.
3. Evaluate soft-subtitle muxing as a separate `embed` command with container-aware codecs.
4. Evaluate `.vtt` and plain transcript outputs without weakening the current SRT contract.
5. Add cross-platform package-manager adapters only behind the existing dependency interface.

Any convenience orchestration should compose existing commands and preserve their standalone
contracts.
