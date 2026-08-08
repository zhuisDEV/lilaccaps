# Contributing

## Scope

Keep the project aligned with the core design:

- `lilaccaps burnin` is render-only
- subtitle generation stays separate from burn-in
- primary flow first, fallback second
- Rust-first implementation unless there is a strong boundary reason to use TS or Deno

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

On macOS:

```bash
brew install ffmpeg-full cmake imagemagick shellcheck actionlint gitleaks
brew link --overwrite --force ffmpeg-full
```

Run the main checks before opening a pull request:

```bash
cargo fmt --all --check
shellcheck install.sh
actionlint
gitleaks detect --source . --no-banner --redact
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
cargo audit
```

Build and smoke-test the release shape with `cargo build --release --locked`. Media-path changes
should be exercised with real FFmpeg inputs, and output files should be checked with `ffprobe`.

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
