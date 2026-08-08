# Security Policy

## Supported Versions

Security fixes are released on the latest stable `v0.1.x` release. Older tags are not maintained
as separate support branches. Confirm the installed and current release with `lilaccaps status`,
then use `lilaccaps update` to upgrade.

## Reporting

Report vulnerabilities privately through
[GitHub Security Advisories](https://github.com/zhuisDEV/lilaccaps/security/advisories/new). Do not
open a public issue containing credentials, private media paths, exploit details, or user data.

Include the lilaccaps version, operating system, relevant command, and a minimal reproduction that
does not contain private media or API keys.

## Security Boundaries

- `GEMINI_API_KEY` is read from the process environment first and then the runtime `.env`. It is
  sent in the `x-goog-api-key` header and must not be logged, committed, or placed in a URL.
- Faster-whisper is optional. Its pinned PEP 723 helper is executed by `uv`, which downloads Python
  packages and the selected model on first use; review the pinned dependency before changing it.
- Codex cleanup is disabled by default because it sends subtitle text to the configured Codex
  provider. It runs with an ephemeral session, a read-only sandbox, a dedicated temporary working
  directory, and strict structured output. Source audio/video and timestamps are not included in the
  prompt. lilaccaps filters subprocess failure text so echoed subtitle prompts are not reported.
- Cleanup output must preserve cue count, indexes, order, and exact timestamps. Empty/multiline text
  and edits that replace more than half of a cue are rejected before any output SRT is published.
- Model downloads use HTTPS, stream to a temporary sibling file, verify the transmitted length when
  available, and are renamed into place only after completion.
- Subtitle and video outputs use temporary files and reject an output that aliases an input through
  a path, symlink, or hard link.
- Temporary audio and overlay assets use unique per-process names and are removed automatically.
- Recursive uninstall requires `--yes`, validates the target before any deletion, and requires an
  ownership marker for custom runtime homes. Protected, shallow, relative, and symlinked directories
  are refused.
- External programs are invoked with structured argument lists rather than shell interpolation.

## Dependency Policy

Release preparation requires locked tests, strict Clippy, ShellCheck, actionlint, gitleaks, and
`cargo audit`. CI pins third-party actions to immutable commits. On macOS, `lilaccaps update`
refreshes the explicitly
managed Homebrew packages (`ffmpeg-full`, `cmake`, and `imagemagick`) before updating the CLI;
`--skip-dependencies` is available for externally managed systems.

## Scope Notes

Areas that deserve extra care:

- shell command invocation
- external binary discovery
- downloaded model artifacts
- file deletion during uninstall
- media path handling and subtitle file parsing
- API credential handling
- optional transcript disclosure through Codex cleanup
- uv-managed Python dependency and model downloads
- release and installer supply-chain changes
