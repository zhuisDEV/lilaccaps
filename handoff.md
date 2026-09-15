# lilaccaps Handoff

## v1.0.0 Release Scope

Deliver an agent-in-the-loop caption project and reusable watermarks as **v1.0.0**. Publication was
authorised on 2026-09-15 after review of the working alpha and its live sample. Keep the current
configurable cue timing defaults; unclear-audio improvements remain later work. The exact release
commit must pass CI before tagging and publication. Published metadata and final CI evidence belong
in the [GitHub release](https://github.com/zhuisDEV/lilaccaps/releases/tag/v1.0.0).

## v1 Implementation

- Added `workflow start/resume/status/accept/render`. Projects retain raw ASR, numbered source and
  translation revisions, structured agent reports, a review queue, config and watermark snapshots.
  Failed stages resume without repeating completed review or losing older manual edits.
- Added separate caption generation and verification requests with stable cue IDs, exact original
  timings, neighbouring cues and joined utterance context. Invalid responses retry in smaller
  batches; subprocess, authentication and timeout failures stop immediately. Translation defaults
  to Luna generation and Terra verification, with both model settings configurable.
- Added conservative transcription correction, common English/Chinese quantity checks, and visible
  uncertainty/readability issues. Agent processes use structured outputs, ephemeral read-only Codex
  sessions and existing ChatGPT authentication. Unix timeouts terminate the entire process group.
- Review acceptance binds source and selected captions, reports and watermark assets. Edits
  invalidate acceptance. Project locks, immutable raw checkpoints, media fingerprints and revision
  validation protect resumability; rendering freezes the accepted inputs and rejects existing output
  paths.
- Added named text/image watermark presets with copied image and explicit font assets. Projects
  keep independent snapshots, so removing a library preset does not break an existing project.
- Combined subtitle and watermark rendering in one successful encode, with portable font discovery
  and retained native/fallback rendering paths. The workflow compares video metadata and every audio
  packet, fully decodes the candidate, and only then publishes a new output file.
- Updated README, skill, generated integration guidance and product plan. New setup defaults to the
  Codex skill directory while preserving configured paths and customised skills. Added a Linux CI
  job alongside macOS, including genuine FFmpeg/ImageMagick rendering tests.
- Updated compatible locked dependency patches; the current RustSec audit is clean.
- Declared Rust 1.89 as the minimum for project file locking and documented the toolchain upgrade
  step for existing installations.
- A release gate exposed transient Linux `ETXTBSY` while launching a newly written executable.
  Only that pre-execution error gets a brief bounded retry, counted inside the request timeout;
  missing executables, permission failures and failed requests still stop immediately. Regression
  tests hold real write-open executable handles to cover recovery and both retry/timeout bounds.
- The Linux CI timing test now checks caption visibility at exact frame indices and validates every
  output frame timestamp. It avoids comparing lossy output pixels with the original and includes a
  deliberately early caption as a negative control, while retaining the rendering implementation.

## Validation: 2026-09-15

Completed locally on Ubuntu with Rust 1.95, FFmpeg 8.0.1, ImageMagick 7.1.2-31 and Codex 0.147.0:

- **195 Rust tests passed**: 175 unit, 3 lifecycle, 5 rendering, 5 translation, 2 watermark-preset
  and 5 workflow tests. Real FFmpeg fixtures cover combined rendering, multiple audio tracks,
  silent video, CJK, PNG/SVG, resumability, stale acceptance and no-clobber output publication.
- Rust formatting, strict all-target/all-feature Clippy, locked tests and optimised release build.
- Python formatting/lint/type checks and the pinned faster-whisper dependency probe.
- Shell syntax, ShellCheck, actionlint, skill validation and `git diff --check`.
- Gitleaks checks of history and the current non-ignored working files found no leaks. Cargo audit
  reported zero vulnerabilities and zero warnings after compatible dependency updates.

### Live Caption Project

The local sample at `.tmp/v1-smoke/job` used a 32-second English excerpt, cached faster-whisper
large-v3-turbo, source review, Simplified Chinese translation, explicit review acceptance and the
saved `LILAC` watermark. The final video is `.tmp/v1-smoke/job/final.mp4`; selected captions are in
`target-0002/captions.srt`, with technical evidence in `render-0001/verification.json`.

- All 21 cue IDs and millisecond timings were retained. A live trial exposed a reversed comparison
  across cues; joined utterance context and the stronger verification pass corrected it while
  retaining `20 trillion` as `20万亿`. The previous revision remains available for comparison.
- The render contains Chinese captions only, native ASS size 32 with the default outline, and a
  top-right `LILAC` watermark. Four representative frames, including the longest caption and the
  financial amount, were visually checked for readable glyphs, placement and clipping.
- The 1920x1080 output retains all 960 frames and the 32.032-second container duration. All 1,501
  audio packets retain their data and timestamps. Full-file decode passed.
- The sample acceptance explicitly acknowledges short cues and an unclear repeated source phrase.
  Review was of the text and sampled visuals, not a full recording/audio accuracy audit.

The sample, logs and local check tools are ignored development artefacts, not release assets. The
input recording, previous user exports, installed stable CLI and installed skill were preserved.

## Release Gates and Limits

- The exact pushed commit must pass GitHub Actions on macOS and Linux before a release. CI
  configuration was also validated locally. See the GitHub release for the final run and commit.
- Text-only review cannot settle unclear speech. Numeric QA is heuristic and timing is preserved;
  short cues are flagged rather than automatically retimed. Audio-grounded review and richer timing
  repair remain later work in the product plan.
- The live model test is one English-to-Chinese excerpt. It demonstrates integration and a concrete
  correction, not general transcription/translation accuracy across languages or recordings.
- Font family presets depend on installed fonts; use explicit font files for portable assets.
  Technical render verification still requires a separate visual check for appearance.

## Historical v0.1.x Notes

The following sections record earlier milestones and their verification at that time.

### Version Reporting

- Replaced the root Clap-only `--version` response with a fail-open release-aware report. It always
  prints the installed version first, uses a five-second stable-release lookup, and only adds
  `new_version` plus a `lilaccaps update` recommendation when semantic-version comparison confirms
  an upgrade.
- Kept `-V` equivalent to `--version`; malformed release tags and network/API failures quietly fall
  back to the installed-version line.
- Added focused root-argument and semantic-version comparison tests and synchronized the README,
  quick-start skill, generated integration guidance, and product plan.

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

At that milestone, the `v0.1.19` publication sequence was complete; `v0.1.20` still required the full
local release gate and checks on the exact pushed commit.

## Release Discipline

Do not publish a new tag until the exact pushed commit passes CI. Stage only reviewed project
changes and verify an installed release from the published tag afterward.
