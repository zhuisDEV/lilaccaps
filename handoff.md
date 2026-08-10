# lilaccaps Handoff

## Unreleased

- Replaced the root Clap-only `--version` response with a fail-open release-aware report. It always
  prints the installed version first, uses a five-second stable-release lookup, and only adds
  `new_version` plus a `lilaccaps update` recommendation when semantic-version comparison confirms
  an upgrade.
- Kept `-V` equivalent to `--version`; malformed release tags and network/API failures quietly fall
  back to the installed-version line.
- Added focused root-argument and semantic-version comparison tests and synchronized the README,
  quick-start skill, generated integration guidance, and product plan.

## Current Objective

Maintain the post-`v0.1.20` CLI and prepare the next `v0.1.x` release from reviewed unreleased work.
Keep transcription, translation, burn-in, and watermarking as separate commands.

## v0.1.20 Transcription Quality Work

- Added configurable overlapping transcription windows; defaults are 30-second windows with two
  seconds of shared context.
- Added default local speech-aware segmentation with adaptive RMS analysis, short-pause bridging,
  isolated-noise rejection, configurable padding, long-silence omission, bounded continuous-speech
  windows, explicit fixed mode, and automatic fixed fallback when no speech is detected.
- Assigned every overlap region to one deterministic window and deduplicated matching cross-window
  cues near boundaries.
- Added deterministic cue normalization: text cleanup, media-duration clamping, minimum and maximum
  durations, end padding, overlap prevention, empty-cue removal, sorting, and renumbering.
- Added proportional text/timing and line-bound cue splitting for long Whisper segments;
  this remains the automatic fallback when timed-token reconstruction is unavailable or unsafe.
- Enabled stable Whisper token timestamps and added UTF-8-safe timed-unit reconstruction with
  recognized-text and Latin-spacing integrity validation before the word-timed path is accepted.
- Added punctuation-, pause-, duration-, and CJK-aware word-timed cue building with balanced hard
  splits and no orphaned punctuation; successful output reports `cue_timing`.
- Added structural SRT QA that fails closed and readability warnings for long lines, excessive line
  counts, long durations, and immediately repeated cue text.
- Added backward-compatible `[transcribe.segmentation]` and `[transcribe.cues]` configuration.
- Added cue, QA, segmentation-strategy, and window counts to successful `transcribe` output, with
  threshold, region, and speech-coverage diagnostics on stderr.
- Added an opt-in faster-whisper 1.2.1 engine through a pinned uv/PEP 723 helper, with
  `large-v3-turbo`/`large-v3`, Silero VAD, word timestamps, managed model caching, and engine-aware
  install/doctor/status checks. `whisper-rs` remains the default.
- Added optional structured Codex cleanup with a read-only ephemeral subprocess in a dedicated
  temporary directory. It can only replace cue text, preserves indexes/order/timestamps, rejects
  missing/multiline/wholesale-rewritten cues, and sanitizes subprocess failures so transcript text is
  not echoed by lilaccaps.
- Added `--engine`, `--model`, and `--cleanup [MODEL]`, plus persistent engine and cleanup config.
- Updated the README, skill guidance, bootstrap/generated-skill text, contributor/security docs,
  product plan, and original architecture proposal. The completed transcription-specific plan is
  removed; living roadmaps with unfinished independent work remain.

Local validation covers 122 Rust tests, strict Clippy, Rust/Python formatting and linting, Python
type checking, the pinned faster-whisper dependency probe, and the locked release build. A
122.7-second Mandarin transcription validated Phase 1
overlap ownership and deduplication. A second 66.0-second Mandarin recording with two- and
three-second silent gaps validated Phase 2: six detected speech regions became three padded windows
at 0.0–20.2, 21.7–42.8, and 45.3–66.0 seconds with 90.1% speech coverage. The final SRT contained 19
continuous, non-overlapping, duration-bounded cues and zero QA warnings. Re-running that fixture
through Phase 3 used 243 timed units across all three windows and produced 23 punctuation- and
pause-aware cues with zero QA warnings and no segment fallbacks. Phase 4 processed the same fixture
with `large-v3-turbo`, Silero VAD, 230 timed words, 23 cues, and zero QA warnings. Phase 5 reached the
configured Codex model but the live provider rejected the request because the workspace was out of
credits; the complete subprocess/schema/timing/rewrite contract is covered by an executable mock and
focused tests. The full local release gate and pushed CI must pass before publication.

## Root Cause Addressed

The reported transcription failure was not caused by the input video. Homebrew had upgraded x265
while a stale linked FFmpeg binary still referenced the removed `libx265.215.dylib`. lilaccaps only
checked whether `ffmpeg` existed on `PATH`, so it accepted a binary that could not launch.

The runtime now executes each dependency's version command. A present but broken executable is
reported as `unhealthy`; `doctor --fix` reinstalls the mapped Homebrew formula, and `update` refreshes
`ffmpeg-full`, `cmake`, and `imagemagick`, relinks FFmpeg Full, validates the toolchain, installs the
locked release, and runs post-update setup validation. `--skip-dependencies` remains available for
externally managed systems.

## v0.1.19 Changes

- Updated all direct Rust dependencies and regenerated `Cargo.lock` with the current Rust toolchain.
- Added dependency executable paths, versions, and startup errors to `doctor` and text/JSON `status`.
- Added a stable GitHub repository fallback so remote installs can self-update outside a checkout.
- Streamed Whisper model downloads to atomic temporary files instead of buffering them in memory.
- Added unique scoped scratch files/directories with cleanup on success and failure.
- Added atomic subtitle/video outputs and same-file protection for paths, symlinks, and hard links.
- Added runtime ownership markers and conservative recursive-uninstall validation.
- Moved Gemini credentials from URL query parameters to `x-goog-api-key`; explicit environment
  values now take precedence over `.env`.
- Replaced the retired `gemini-3.1-flash-lite-preview` default with
  `gemini-3.1-flash-lite` and migrate the exact legacy managed default during install/update.
- Made SRT parsing accept BOM, CRLF, flexible arrow spacing, and dot milliseconds while rejecting
  reversed/overflowing timing; Whisper cues are clamped at chunk boundaries.
- Routed ImageMagick caption/watermark text through owned files so `@path` remains literal text.
- Made the remote installer invoke the exact Cargo-installed binary even when it is not on `PATH`.
- Hardened CI with pinned current actions, FFmpeg Full, ShellCheck, actionlint, gitleaks, strict
  Clippy, locked tests, and RustSec audit.
- Synchronized README, quick-start skill, contributing guide, security policy, product plan, and
  historical proposal status.

## Verification State

Completed locally:

- FFmpeg Full 8.1.2, FFprobe 8.1.2, CMake 4.4.2, ImageMagick 7.1.2-29
- Homebrew linkage checks for FFmpeg Full and ImageMagick
- 84 unit tests
- strict Clippy with all targets/features and locked dependencies
- Rust formatting
- `sh -n`, ShellCheck, actionlint, gitleaks, and Markdown link validation
- clean `cargo audit` over 210 locked crates and no pending `cargo update`
- locked optimized release build reporting `lilaccaps 0.1.19`
- real 79.3-second music transcription and focused 42.3-second speech transcription
- verified no overlap at the 30-second Whisper chunk boundary
- FFmpeg/libass and ImageMagick burn-in paths plus text, PNG, and SVG watermark paths
- visual frame inspection, `ffprobe` stream/duration checks, and unchanged-input SHA-256 checks
- local install, config migration, ownership marker, generated skill, and bootstrap refresh

Translation request construction, credential precedence, parsing, and missing-credential failure are
covered, but a live Gemini translation was not run because this machine has no `GEMINI_API_KEY`.

The `v0.1.19` publication sequence is complete. Publish `v0.1.20` only after the full local release
gate and exact pushed commit pass.

## Release Discipline

Do not publish a new tag until the exact pushed commit passes CI. Stage only reviewed project
changes and verify an installed release from the published tag afterward.
