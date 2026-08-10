# lilaccaps Product and Engineering Plan

## Purpose

`lilaccaps` is the canonical Rust CLI for local subtitle and video-rendering workflows. Each command
owns one explicit artifact transformation; lifecycle work remains separate from media processing.

## Current Product

Implemented commands:

```text
lilaccaps --version
lilaccaps doctor [--fix]
lilaccaps install [--fix]
lilaccaps update [--skip-dependencies]
lilaccaps status [--json]
lilaccaps uninstall --yes
lilaccaps transcribe <media> [--lang <code>] [--engine <engine>] [--model <model>] [--cleanup [model]] [--output <file.srt>]
lilaccaps translate <file.srt> --to <language> [--append <bool>]
lilaccaps burnin <video> --subs <file.srt> [--output <video>]
lilaccaps watermark <video> (--text <text> | --image <path>) [--output <video>]
```

Current capabilities:

- fail-open installed-version reporting with stable-release comparison and update guidance
- local `whisper-rs` and uv-managed faster-whisper transcription with automatic or explicit
  language selection
- overlapping transcription windows with deterministic boundary ownership and deduplication
- local speech-aware segmentation with padding, pause bridging, bounded windows, and fixed fallback
- Whisper token timestamps with punctuation-, pause-, and CJK-aware cue rebuilding plus safe segment
  fallback
- cue timing normalization plus structural and readability QA before SRT publication
- higher-quality `large-v3`/`large-v3-turbo` transcription with Silero VAD and word timestamps
- optional conservative Codex text cleanup with exact structural preservation and fail-closed
  rewrite validation
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
- Optional faster-whisper runtime: `uv` plus a pinned PEP 723 helper environment
- Optional cleanup runtime: authenticated `codex`; subtitle text is sent to its configured provider

Paths beginning with `~` are expanded after config parsing. Explicit environment values take
precedence over file-based defaults.

## Architecture

- `src/commands`: argument-to-pipeline dispatch and structured command summaries
- `src/pipelines`: one module per media workflow
- `src/media.rs`: FFmpeg/FFprobe boundaries and media probing
- `src/segmentation.rs`: local speech analysis and bounded transcription-window planning
- `src/model.rs`: engine-aware model/cache resolution and atomic whisper-rs model download
- `src/faster_whisper.rs` and `python/faster_whisper_helper.py`: uv-managed faster-whisper boundary
- `src/cleanup.rs`: isolated structured Codex cleanup and conservative output validation
- `src/subtitles.rs`: timed-word cue building, SRT timing optimization, QA, parsing, and atomic
  serialization
- `src/render.rs`: burn-in renderer selection and overlay implementation
- `src/watermark.rs`: text/image watermark renderer selection
- `src/runtime.rs`: dependencies, paths, temporary assets, ownership, and lifecycle safety
- `src/config.rs`, `src/release.rs`, `src/integration.rs`: config migration, release discovery, and
  OpenClaw integration

## Release Gate

Every release must complete:

```bash
cargo fmt --all --check
uvx ruff format --check python
uvx ruff check python
uvx ty check python
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
