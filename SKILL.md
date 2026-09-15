---
name: lilaccaps
description: Use lilaccaps to transcribe media, review and translate captions with an agent, resume caption projects, burn reviewed subtitles into video, and save or reuse text/image watermarks. Also use for lilaccaps health checks, settings and updates.
---

# LilacCaps

Use the installed CLI for media work. For a complete captioning task, prefer the v1 `workflow`
commands. Keep the user's requested stopping point: an SRT request stops at captions; rendering
requires the user's requested scope to include burn-in. The workflow's review checkpoint records
review of exact files; it does not require asking again when the user has already authorised
rendering and the necessary review has been completed.

## Find the environment

- Full manual: [README.md](README.md), or the [repository manual](https://github.com/zhuisDEV/lilaccaps#readme).
- Runtime/config: `~/.lilac/lilaccaps/lilaccaps.toml`, overridden by `LILACCAPS_HOME` or `--config-path`.
- Models and watermark presets: `models/` and `watermarks/` under the configured runtime home.
- Preserve the configured `agent.skill_path`; it may belong to Codex or OpenClaw.
- Check `lilaccaps workflow --help` before using v1 commands with an older installation.
- Building or updating v1 requires Rust 1.89 or newer; update the toolchain first if necessary.
- Use `lilaccaps status` or `doctor` when readiness is uncertain; `--version` also checks for a newer stable release.
- `doctor --fix` and dependency updates target macOS/Homebrew. On externally managed systems,
  use `lilaccaps update --skip-dependencies` when an update is requested.
- Older Linux updaters through 1.0.0 can replace the binary and then report `failed to resolve the
  installed executable`. Verify the newly installed version, then finish with `lilaccaps install`
  using the same config path. Version 1.0.1 fixes subsequent self-updates.

## Caption project

```bash
lilaccaps workflow start input.mp4 --project captions --engine faster-whisper --model large-v3-turbo --to zh-hans --watermark lilac
lilaccaps workflow status captions --json
```

The watermark must already exist; omit `--watermark` when none is requested. `--to` is optional;
with a target only the translated captions are selected for rendering. Use `--subs input.srt` to
reuse an existing transcript. Supply names and terminology with `--context-file glossary.txt`
(at most 32,000 characters and 64 KiB). Engine/model/language otherwise use existing configuration.
Faster-whisper requires `uv` and may download its model on first use.

The workflow keeps raw captions, numbered revisions, agent reports, and `review.md`, then stops at
`review_required`. Generation and verification are separate Codex calls with stable cue IDs, neighbouring cues and
joined source utterances. Translation defaults to Luna generation and Terra verification; configure
`translate.review_model` and `translate.review_reasoning_effort` independently when needed. They send subtitle text and glossary context using the existing ChatGPT login;
they do not send the recording. They consume account usage separately.

### Perform the review

1. Read `review.md`, both source and selected SRT, and the per-revision `review.json` files.
2. Check names, numbers/currency magnitudes, negation, sentence continuations, duplicated or omitted
   meaning, and target-language naturalness. Number checks are heuristic. An empty issue list is not proof.
3. Check uncertain ASR wording against the recording when possible. Text-only model review cannot
   establish what was spoken; preserve unresolved uncertainty and state what could not be verified.
4. Inspect short cues and wrapping. Keep target IDs and millisecond timestamps aligned to the source.
   Edit the selected SRT as needed. If source captions change after translation, run `workflow resume`
   to create a new target revision; older translations and manual edits are retained.
5. When review is complete and rendering is in scope, record an honest note and render:

```bash
lilaccaps workflow accept captions --note "Describe the checks performed and remaining uncertainty."
lilaccaps workflow render captions --font "Noto Sans CJK SC" --size 32
```

Use a font installed on the computer. Native FFmpeg/libass size uses ASS script units: `32` is twice
its usual SRT default `16`, not 32 screen pixels. ImageMagick overlays use point size. SRT itself
stores no font size. The default black outline width is `2`.

Rendering combines captions and the saved watermark in one successful encode, copies audio, and
verifies dimensions/duration/available frame counts, audio packets and strict full decode before
publishing. Inspect representative rendered frames, including the longest caption, for clipping,
placement and watermark appearance. Technical checks do not replace visual or linguistic review.
Report the final paths and verification limits. Caption/watermark edits invalidate earlier acceptance.
Use a new `--output` path when rendering another copy; existing output files are preserved.

`workflow resume` finishes missing stages after an error. It does not redo a completed source pass
when only translation failed. Use `resume --review-source` to review current source captions again,
or `resume --retranslate` to generate a new target revision after changing model settings.
`workflow status --json` gives paths, issues and the next action.

## Reusable watermark

```bash
lilaccaps watermark-preset save lilac --text LILAC --position top-right --size 48 --opacity 0.7 --margin 40
lilaccaps watermark-preset save logo --image logo.png --size 180 --position top-right
lilaccaps watermark-preset list --json
lilaccaps watermark-preset show lilac --json
lilaccaps watermark input.mp4 --preset lilac --output branded.mp4
```

Presets copy images and explicit font files into owned storage. Names use lowercase letters,
digits, hyphens or underscores; duplicate saves fail. Project copies survive removal of library
presets. Do not combine `--preset` with inline text/image/style flags; save a differently named variant.
Use `watermark-preset remove NAME` only when removal is requested.

## Individual transformations

```bash
lilaccaps transcribe input.mp4 --lang en --engine faster-whisper --model large-v3-turbo --output draft.srt
lilaccaps transcribe input.mp4 --cleanup --output reviewed.srt
lilaccaps translate draft.srt --to zh-hans --append false --output chinese.srt
lilaccaps translate draft.srt --to zh-hans --to en --append true --output multilingual.srt
lilaccaps burnin input.mp4 --subs chinese.srt --font "Noto Sans CJK SC" --size 32 --output captioned.mp4
lilaccaps watermark input.mp4 --text LILAC --position top-right --output branded.mp4
```

Standalone transcription remains local by default. `--cleanup` enables conservative text generation
and verification using `transcribe.cleanup` settings. Translation uses `translate` model/effort settings
and writes `<output.srt>.review.json`; inspect it with the SRT. Invalid structured responses retry in
bounded smaller batches; process/authentication/timeout failures stop. Inputs are never outputs.

Rendering commands transform supplied files only. Prefer explicit output paths and preserve source
recordings, editable SRTs, and older user-reviewed copies. See the manual for outline, colour,
multilingual styles, engine settings and installation.
