# Contributing

## Scope

Keep the project aligned with the core design:

- `lilaccaps burnin` is render-only
- subtitle generation stays separate from burn-in
- primary flow first, fallback second
- Rust-first implementation unless a narrow external-runtime boundary is justified; the
  faster-whisper helper stays pinned, self-contained, and uv-managed

## Development Setup

Required tools:

- `cargo`
- `ffmpeg`
- `ffprobe`
- `cmake`
- `magick`
- `shellcheck`
- `actionlint`
- `gitleaks`
- `uv`
- `ruff` and `ty` through `uvx`

On macOS:

```bash
brew install ffmpeg-full cmake imagemagick shellcheck actionlint gitleaks uv
brew link --overwrite --force ffmpeg-full
```

Run the main checks before opening a pull request:

```bash
cargo fmt --all --check
uvx ruff format --check python
uvx ruff check python
uvx ty check python
uv run --script python/faster_whisper_helper.py --audio python/faster_whisper_helper.py --model large-v3-turbo --download-root /tmp/lilaccaps-model-check --check
shellcheck install.sh
actionlint
gitleaks detect --source . --no-banner --redact
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
cargo audit
```

Build and smoke-test the release shape with `cargo build --release --locked`. Media-path changes
should be exercised with real FFmpeg inputs, and output files should be checked with `ffprobe`.
Changes to the faster-whisper path require a real uv-managed transcription fixture; cleanup changes
require subprocess/schema tests that prove cue indexes, order, timestamps, and failure behavior.

## Pull Requests

- keep changes focused
- avoid mixing primary and fallback logic in the same branch when a cleaner split is possible
- update docs when command behavior changes
- include verification notes in the PR description
- update `Cargo.lock` intentionally and keep all release builds locked

## Code Style

- keep modules small and explicit
- prefer direct command semantics over hidden behavior
- preserve the separation between transcription and rendering
