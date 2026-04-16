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
- Forced transcription language should retry with the detected language when Whisper returns no usable segments and detection identifies a different language.
- The ImageMagick burn-in fallback must emit `PNG32:` overlays and normalize overlay inputs to `rgba`; otherwise ffmpeg can compose monochrome/transparent inputs into a black video stream.
- For CJK subtitles in the ImageMagick fallback, prefer a CJK-capable macOS system font and render text via a `label:` image before expanding to the video canvas; the older full-canvas annotate path could silently produce transparent overlays.
- Burn-in font choice should key off the subtitle text itself: CJK-heavy cues should prefer CJK-capable fonts, while Latin text can stay on the lighter Latin fallback.
- Transcription should fall back across both language selection and decoding strategy: try beam search first, then greedy decoding, before treating a clip as having produced no subtitle text.
- Whisper model resolution now supports `medium` and `medium.en` in addition to the smaller model sizes.
- Empty-output transcription failures now include per-attempt diagnostics showing language, decode strategy, total segments, non-empty segments, and blank-segment counts.
- Auto-language transcription now performs a dedicated detection step and then transcribes with the detected language explicitly, avoiding whisper.cpp's detect-only early return and reporting the effective language in command output.
- Explicit `--lang` retries should not depend on language detection succeeding first; detected-language retries are optional fallback attempts, not a prerequisite for the requested language path.
- README and generated bootstrap docs should distinguish minimum `ffmpeg` support from the recommended Homebrew `ffmpeg-full` install for native libass-based burn-in.
- Successful `transcribe` runs should stay quiet: ffmpeg audio extraction now runs with `-hide_banner -loglevel error`, and Whisper backend logs are routed through `whisper-rs` logging hooks so normal runs only print the CLI summary.
- Burn-in style now supports CLI overrides plus TOML defaults for `font` and `size`; `font = "auto"` keeps renderer font selection, and `size = 0` keeps automatic height-based scaling.
- `burnin.line_spacing` is now a TOML-only renderer setting; `0` means auto spacing, and a positive value forces the overlay renderer so multiline spacing can be honored consistently.
- Translation is a separate workflow: `translate` rewrites cue text but preserves cue timing and indexes exactly, so `burnin` can stay render-only.
- Translation defaults now live under `[translate]` in `lilaccaps.toml`, and Gemini API credentials are loaded from `GEMINI_API_KEY` in the environment or from `$LILACCAPS_HOME/.env`.
- Multilingual captions are represented as multi-line cue text, with `translate --append` adding one translated line per target language under the original subtitle text.
- `translate.line_order` now controls the written top-to-bottom order of multilingual cue lines, and `burnin.styles.<role>` uses those same role labels to apply per-language font and size during rendering.
- If explicit per-language fonts look worse than the renderer defaults, set `burnin.styles.<role>.font = "auto"` (or omit the per-role font) and keep only the per-language sizes.
- The ImageMagick multiline fallback must create each `label:` image before applying border/stacking operations; otherwise `magick` can fail with `no images found for operation '-border'`.
- The multiline ImageMagick fallback should keep vertical padding tight; large fixed borders make multilingual subtitle stacks look too loose.
