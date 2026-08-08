# lilaccaps Handoff

## Current Objective

Prepare and publish `v0.1.19` as a dependency-health, lifecycle-safety, and reliability release.
Keep transcription, translation, burn-in, and watermarking as separate commands.

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

Publication sequence:

- Commit and push the reviewed release scope.
- Wait for the exact pushed commit to pass GitHub Actions.
- Create and push the annotated tag, then publish the GitHub release.
- Install/update from the published tag and complete final local health verification.

## Release Discipline

Do not publish a tag until the exact pushed commit passes CI. Stage only reviewed project changes,
use an annotated `v0.1.19` tag, publish release notes that explain the FFmpeg/x265 root cause, and
verify the installed binary reports `0.1.19` afterward.
