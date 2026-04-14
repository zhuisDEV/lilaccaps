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

Run the main checks before opening a pull request:

```bash
cargo fmt
cargo test
```

## Pull Requests

- keep changes focused
- avoid mixing primary and fallback logic in the same branch when a cleaner split is possible
- update docs when command behavior changes
- include verification notes in the PR description

## Code Style

- keep modules small and explicit
- prefer direct command semantics over hidden behavior
- preserve the separation between caption generation and rendering
