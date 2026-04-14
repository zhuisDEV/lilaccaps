# lilaccaps Implementation Handoff

## Goal
Implement a clean `lilaccaps` CLI with two separate primary workflows:

1. transcribe video/audio into subtitle output
2. burn an existing subtitle file into a video

Do not collapse these into one command.

## Fixed Decisions
- Project and CLI name is `lilaccaps`
- CLI is unified under `lilaccaps`
- `lilaccaps burnin` is render-only
- Caption generation is a separate command/workflow
- Video/audio to transcription may be delegated by the main OpenClaw agent to a subagent
- Use Rust/TS/Deno
- Keep the primary flow clean
- Add fallback only after the main path is solid

## Recommended First Cut
Define two commands only:

```text
lilaccaps transcribe <input>
lilaccaps burnin <video> --subs <subtitle-file>
```

## Implementation Boundary

### `transcribe`
Owns:
- media probing
- audio extraction or normalization if needed
- transcription
- subtitle cleanup
- `.srt` output

Does not own:
- video burn-in rendering

### `burnin`
Owns:
- validating existing subtitle input
- rendering subtitles into video
- writing final rendered video

Does not own:
- transcription
- subtitle generation

## Engineering Notes
- Keep command handlers thin
- Move real work into pipeline modules
- Keep primary backend selection straightforward
- Put fallback backends behind adapters later, not in the initial command logic
- Return exact output paths on success
- Keep error messages concrete and file-oriented

## Suggested Build Order
1. Scaffold Rust CLI entrypoint `lilaccaps`
2. Add `transcribe` and `burnin` subcommands
3. Implement `.srt` generation pipeline
4. Implement burn-in rendering pipeline
5. Add subagent delegation path for transcription if the surrounding OpenClaw integration needs it

## What To Avoid
- making `burnin` auto-generate subtitles
- mixing transcription fallback with the primary transcription path
- creating a convenience mega-command before core flows are stable
- coupling agent orchestration directly to CLI command semantics

## Deliverable Standard
The next implementation should leave the repo with:
- a clear CLI surface
- separate caption and render modules
- exact artifact paths in output
- room to add fallback cleanly later

## Current Handoff Notes
- Native tool prerequisites are now surfaced explicitly in CLI status and preflight checks.
- `cargo install` still cannot be intercepted by the CLI before compilation, so README guidance must stay accurate and prominent for `cmake`.
- `status` distinguishes core runtime health from build/update readiness and fallback renderer readiness.
- Runtime dependency tests should use synthetic missing commands so they remain stable across developer machines.
- `doctor` is the shared prerequisite inspection entrypoint; `install` should reuse it instead of reintroducing bespoke dependency checks.
- Automatic prerequisite repair is intentionally limited to macOS plus Homebrew and only for explicitly mapped packages.
- The default config path now lives under the runtime home at `~/.lilac/lilaccaps/lilaccaps.toml`.
- Transcription language now supports config default plus per-run CLI override via `--lang`; `"auto"` preserves Whisper auto-detection.
- Config schema changes must stay backward-compatible on read; older configs missing `transcribe.language` should default to `"auto"` instead of breaking `update`.
