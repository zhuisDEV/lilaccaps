# LilacCaps v1 Product and Engineering Plan

## Objective

Deliver **v1** with an agent-in-the-loop flow from transcription through translation and burn-in,
plus a reusable watermark library. Version 1.0.0 is published; the 1.0.1 follow-up fixes a Linux
self-update failure found during the isolated upgrade check. Publication is authorised and gated
by local checks and CI on the exact pushed commit.

## Scope and decisions

- Keep standalone `transcribe`, `translate`, `burnin` and `watermark` commands.
- Add explicit `workflow start/resume/status/accept/render` orchestration. A new project retains raw
  ASR, source review, translated revisions, review notes, asset snapshots and verification evidence.
- Text-only Codex generation followed by independent verification uses stable cue IDs, exact timing,
  bounded contextual batches and malformed-output recovery. Uncertainty stays visible; the agent
  cannot approve its own output or claim to have heard audio.
- Review acceptance binds current captions and watermark assets. Edits require new acceptance.
  Source edits after translation create a new target revision; older files are preserved.
- Rendering combines captions and watermark in one successful encode, then verifies media integrity
  before atomic no-clobber publication. Visual/linguistic review remains explicit.
- Named text/image watermarks retain copied image/font assets. Snapshots survive library removal.
- Preserve existing renderer size semantics; document native ASS units versus overlay point size.
- Retain the current configurable cue timing defaults, as accepted during review. Unclear-audio
  improvements remain future model/review work and do not block this release.
- Support Linux font discovery while retaining platform fallbacks and explicit family/file choices.

## Work and evidence

Implementation and acceptance checks are recorded in [handoff.md](handoff.md). The release candidate
must pass offline cue-protocol/failure-path tests, resumability and stale-acceptance tests, disposable
real FFmpeg rendering checks, and a live model smoke test before release review.

## Architecture

- `src/caption_agent.rs`: cue-ID protocol, contextual generation/verification, heuristic number QA.
- `src/translate.rs`: bounded isolated Codex subprocess with ChatGPT authentication.
- `src/cleanup.rs`: conservative transcript-edit guard.
- `src/workflow.rs`: persisted checkpoints, revisions, acceptance, output publication.
- `src/watermark_presets.rs`: versioned named presets and owned assets.
- `src/render.rs`, `src/fonts.rs`: combined rendering, renderer choice and portable font resolution.
- `src/verification.rs`: ffprobe metadata/audio packet comparison and strict decode.
- Existing `commands`, `pipelines`, `subtitles`, `runtime`, `config` and lifecycle modules remain.

## Runtime contract

- Config: `~/.lilac/lilaccaps/lilaccaps.toml`; override with `LILACCAPS_HOME` or `--config-path`.
- Runtime models, temporary files and presets: `models/`, `tmp/`, `watermarks/`.
- Projects are explicit directories outside runtime cleanup, next to the input by default.
- Agent access reuses authenticated Codex/ChatGPT. No Gemini API key or `.env` credential is used.
- Existing `agent.skill_path` values are preserved; new setup supports Codex and OpenClaw.

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

## Later work

- More languages and domain terminology in numeric/semantic QA; evaluate against labelled recordings.
- Optional audio-grounded review of uncertain spans and richer timing repair.
- Model integrity metadata/checksums, soft-subtitle muxing, VTT and plain transcript export.
- Additional package-manager adapters behind the existing dependency interface.
