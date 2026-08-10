# lilaccaps

[![CI](https://github.com/zhuisDEV/lilaccaps/actions/workflows/ci.yml/badge.svg)](https://github.com/zhuisDEV/lilaccaps/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://rust-lang.org/)

`lilaccaps` is a clean Rust CLI for separate subtitle and video rendering workflows:

1. transcribe local video or audio into subtitle files
2. translate an existing subtitle file into one or more target languages
3. burn an existing subtitle file into a video
4. apply a text or image watermark to a video

The command model stays explicit:

- `lilaccaps --version` reports the installed version and recommends `lilaccaps update` when a newer
  stable release is available
- `lilaccaps doctor` inspects local prerequisites and setup health
- `lilaccaps doctor --fix` installs or repairs unhealthy macOS packages through Homebrew
- `lilaccaps update` updates managed Homebrew dependencies before updating the CLI
- `lilaccaps transcribe` generates subtitle artifacts such as `.srt`
- `lilaccaps translate` adds one or more translated lines to subtitle cues
- `lilaccaps burnin` is render-only and never hides transcription inside the command
- `lilaccaps watermark` applies text or image watermarks without touching subtitles

## Project Cards

| Transcribe | Translate | Burn-in | Watermark | Lifecycle |
| --- | --- | --- | --- | --- |
| Extract audio with `ffmpeg`, transcribe with `whisper-rs` or faster-whisper, write `.srt` | Append multilingual lines to existing cues via Gemini | Render an existing subtitle file into video | Apply text or image watermarks with ffmpeg plus ImageMagick fallback | Install, status, update, and uninstall support |
| Local-first subtitle timing, VAD, QA, and optional text-only Codex cleanup | Cue timing and indexes stay unchanged | Primary renderer uses `ffmpeg` subtitle filters when available | Position, opacity, size, and margin are explicit | Configured with `lilaccaps.toml` and `LILACCAPS_HOME` |
| Output path is explicit | Translation output is explicit | Fallback renderer uses ImageMagick overlays when needed | Output path is explicit | OpenClaw skill bootstrap is supported |

## Features

- Unified CLI under `lilaccaps`
- `transcribe`, `translate`, `burnin`, and `watermark` kept as separate first-class workflows
- Managed Whisper model/cache storage under the runtime home
- Rust-native `whisper-rs` default plus an opt-in uv-managed faster-whisper 1.2.1 backend with
  `large-v3`/`large-v3-turbo`, Silero VAD, and word timestamps
- Optional conservative Codex text cleanup that preserves cue count, order, and timing and rejects
  malformed or wholesale rewritten results
- Streamed, atomic model downloads and atomic media/subtitle outputs
- Runtime health checks through `lilaccaps status`
- Automatic Homebrew dependency refresh during `lilaccaps update`
- Repo-root `SKILL.md` quick start plus this README as the full manual
- Generated OpenClaw skill bootstrap
- Clean primary and fallback separation for rendering
- `.env` support for translation API credentials

## Quick Start and Manual

- [SKILL.md](./SKILL.md) is the quick start for basic settings, common CLI usage, and where to find advanced options.
- This README is the full manual for installation, command behavior, configuration, renderer choices, and troubleshooting.

## Requirements

- Rust toolchain with `cargo`
- `ffmpeg`
- `ffprobe`
- `cmake`
- ImageMagick `magick`

Optional transcription tools:

- `uv` for the `faster-whisper` engine; the pinned helper environment is created on demand
- an authenticated `codex` CLI for `transcribe.cleanup.enabled = true` or `--cleanup`

For native `burnin` rendering, prefer an `ffmpeg` build that includes the `subtitles` or
`ass` filters through `libass`. On macOS with Homebrew, `ffmpeg-full` is the reliable option.
Without those filters, `lilaccaps` falls back to ImageMagick overlay rendering.

On macOS with Homebrew:

```bash
brew install ffmpeg-full cmake imagemagick
```

Install `uv` as well when using faster-whisper:

```bash
brew install uv
```

## Install

Recommended: install globally from the GitHub repo and repair/install mapped macOS dependencies:

```bash
curl -fsSL https://raw.githubusercontent.com/zhuisDEV/lilaccaps/main/install.sh | sh -s -- --fix
```

The remote installer is the recommended install path. It installs the `lilaccaps` binary with
Cargo and then runs `lilaccaps install` to initialize the runtime config, model directory, and
skill bootstrap files. It invokes the exact binary Cargo installed, so the first install also works
when Cargo's bin directory is not yet on `PATH`.

To leave system dependencies untouched, omit `--fix`:

```bash
curl -fsSL https://raw.githubusercontent.com/zhuisDEV/lilaccaps/main/install.sh | sh
```

Set `LILACCAPS_INSTALL_ROOT` to an absolute path to use a Cargo install root other than
`CARGO_HOME` or `~/.cargo`.

If you prefer not to pipe a remote script into `sh`, run the same primary install directly:

```bash
brew install ffmpeg-full cmake imagemagick
cargo install --git https://github.com/zhuisDEV/lilaccaps.git --locked --force lilaccaps
"${CARGO_HOME:-$HOME/.cargo}/bin/lilaccaps" install
```

For local development, build from a checkout and install with cargo:

```bash
brew install ffmpeg-full cmake imagemagick
cargo install --path . --locked
"${CARGO_HOME:-$HOME/.cargo}/bin/lilaccaps" install
```

If `cmake` is missing, `cargo install` fails while compiling `whisper-rs-sys` before the
`lilaccaps` binary exists. Install prerequisites first, then rerun `cargo install`.

`doctor` executes each dependency's version command, so a binary with broken dynamic-library
links is reported as unhealthy instead of available. On macOS, `doctor --fix` installs or
reinstalls the mapped package and prefers `ffmpeg-full` for native subtitle rendering.

After the binary exists, you can inspect or repair prerequisites with:

```bash
lilaccaps doctor
lilaccaps doctor --fix
lilaccaps install --fix
```

`--fix` currently supports macOS through Homebrew and repairs only packages that `lilaccaps`
maps explicitly.

After installation, `lilaccaps status` reports:

- core runtime readiness (`healthy`, `missing`)
- build/update readiness (`cargo_available`, `cmake_available`, `build_ready`)
- fallback renderer readiness (`magick_available`, `fallback_renderer_ready`)
- optional engine readiness (`uv_available`, `transcription_engine_ready`)
- optional cleanup readiness (`codex_available`, `cleanup_ready`)
- fixability (`brew_packages`, `can_fix_with_brew`)
- dependency probe status, resolved executable path, detected version, and startup error
- release-check errors separately from the last known version fields

## Update

Check the installed version and whether a newer stable release is available:

```bash
lilaccaps --version
```

The installed version is always printed first. When the release check confirms a newer semantic
version, the command also prints `new_version` and recommends `lilaccaps update`. The release check
uses a short timeout and fails open, so offline or unavailable GitHub access never prevents the
installed version from being reported.

Update lilaccaps and its managed runtime dependencies:

```bash
lilaccaps update
```

On macOS with Homebrew, the update flow refreshes package metadata, installs or upgrades
`ffmpeg-full`, `cmake`, and `imagemagick`, relinks `ffmpeg-full`, validates every dependency,
installs the latest stable lilaccaps release, and runs the new binary's install validation to
refresh generated bootstrap/skill files. This repairs ABI mismatches such as an
older ffmpeg binary referring to a removed x265 dynamic library.

Skip system dependency updates when they are managed separately:

```bash
lilaccaps update --skip-dependencies
```

Rust crate dependencies are compiled into lilaccaps and locked by each release. They are
updated and tested when a new lilaccaps release is prepared, not rewritten on an end-user
machine at runtime.

`update` also migrates managed config defaults when an upstream preview is retired. Explicit
custom values remain configurable in `lilaccaps.toml`.

This initializes:

- `~/.lilac/lilaccaps/lilaccaps.toml`
- runtime home, defaulting to `~/.lilac/lilaccaps`
- model assets/cache for the configured transcription engine under the runtime home
- OpenClaw skill bootstrap files

For translation, create a local `.env` file under the runtime home, for example
`~/.lilac/lilaccaps/.env`, and set:

```bash
GEMINI_API_KEY=your_google_auth_key_here
```

An exported `GEMINI_API_KEY` takes precedence over `.env`; local files never override an explicit
process environment value. The key is sent in the `x-goog-api-key` request header rather than in
the request URL. Use a current [Gemini API key](https://ai.google.dev/gemini-api/docs/api-key).

## Usage

Transcribe into subtitles:

```bash
lilaccaps transcribe ./input.mp4
lilaccaps transcribe ./input.mp4 --lang zh
lilaccaps transcribe ./input.mp4 --lang zh --engine faster-whisper --model large-v3-turbo
lilaccaps transcribe ./input.mp4 --lang zh --engine faster-whisper --cleanup
```

Translate an existing `.srt` into one or more target languages:

```bash
lilaccaps translate ./input.srt --to en --append
lilaccaps translate ./input.srt --to en --to ja --append
```

That produces multilingual cue text such as:

```text
原文中文
English translation
日本語訳
```

Burn in an existing subtitle file:

```bash
lilaccaps burnin ./input.mp4 --subs ./input.srt
lilaccaps burnin ./input.mp4 --subs ./input.srt --font "PingFang SC" --size 42
lilaccaps burnin ./input.mp4 --subs ./input.srt --outline-colour black --outline-width 3
```

`lilaccaps burnin` reports the actual renderer it used:
- `renderer = ffmpeg-subtitles`
- `renderer = overlay-fallback`

When the overlay fallback is used, `renderer_reason` shows why, for example
`per_line_styles,line_spacing,colour` or `outline_colour`.

Apply a watermark to a video:

```bash
lilaccaps watermark ./input.mp4 --text "lilac"
lilaccaps watermark ./input.mp4 --text "lilac" --position top-right --opacity 0.35 --size 42
lilaccaps watermark ./input.mp4 --text "Lilac Captions" --font Verdana --colour "#E19CFF" --outline-width 2
lilaccaps watermark ./input.mp4 --image ./logo.png --position bottom-right --opacity 0.45 --size 180
lilaccaps watermark ./input.mp4 --image ./logo.svg --position top-left --opacity 0.45
```

`watermark` accepts exactly one of `--text` or `--image`. `--position` supports `top-left`,
`top-right`, `bottom-left`, `bottom-right`, and `center`. For text watermarks, `--size` is the
font size and `0` means the default size. For image watermarks, `--size` is the target image width
and `0` keeps the original image width. `--margin` controls the edge offset. Text watermarks use
the `ffmpeg` `drawtext` filter when available, and automatically fall back to rendering the text as
a PNG with ImageMagick before applying the existing image overlay path when `drawtext` is missing
or fails. SVG image watermarks are also converted to PNG with ImageMagick before overlaying when
needed. `--outline-width` and `--outline-colour`/`--outline-color` add a text watermark outline.
Common font names such as `Arial`, `Verdana`, `Helvetica`, and `PingFang SC` are resolved to font
files for the ImageMagick fallback.

Check environment health:

```bash
lilaccaps status
lilaccaps doctor
```

All generated subtitle and video outputs are written through a temporary path and renamed only
after successful completion. Commands reject an output that resolves to an input file, including
through a symlink or hard link. Per-run audio, overlay, and watermark scratch files are uniquely
named and removed after both successful and failed runs.

## Configuration

The default config file is `~/.lilac/lilaccaps/lilaccaps.toml`.

Important values:

- `runtime.home`
- `agent.skill_path`
- `release.github_repo`
- `transcribe.engine`
- `transcribe.language`
- `transcribe.segmentation.chunk_seconds`
- `transcribe.segmentation.overlap_seconds`
- `transcribe.segmentation.mode`
- `transcribe.segmentation.min_speech_ms`
- `transcribe.segmentation.min_silence_ms`
- `transcribe.segmentation.padding_ms`
- `transcribe.segmentation.max_window_seconds`
- `transcribe.cues.min_duration_ms`
- `transcribe.cues.max_duration_ms`
- `transcribe.cues.end_padding_ms`
- `transcribe.cues.pause_split_ms`
- `transcribe.cues.max_chars_per_line`
- `transcribe.cues.max_cjk_chars_per_line`
- `transcribe.cues.max_lines`
- `transcribe.model.id`
- `transcribe.model.path`
- `transcribe.cleanup.enabled`
- `transcribe.cleanup.command`
- `transcribe.cleanup.model`
- `translate.model`
- `translate.append`
- `translate.default_targets`
- `translate.line_order`
- `burnin.font`
- `burnin.colour`
- `burnin.size`
- `burnin.line_spacing`
- `burnin.advanced_styling`
- `burnin.outline.enabled`
- `burnin.outline.colour`
- `burnin.outline.width`

`LILACCAPS_HOME` overrides the runtime home at runtime.
Runtime, skill, and explicit model paths must be absolute or begin with `~`; relative managed paths
are rejected so lifecycle commands cannot change meaning with the current working directory.

`transcribe.language` defaults to `"auto"`. Set it to a Whisper language code such as
`"en"`, `"zh"`, or `"ja"` to force transcription in a specific language. `--lang` on
`lilaccaps transcribe` overrides the config value for a single run.

With the default `whisper-rs` engine, `language = "auto"` samples the first 30 seconds for language
detection and then transcribes with the detected language explicitly. That avoids the
`detect_language=true` detect-only path in `whisper.cpp`. Faster-whisper performs its own language
detection when no language is forced. Both engines report the effective language.

When you force a language on `whisper-rs`, `lilaccaps` tries:

- requested language with beam search
- requested language with greedy decoding
- detected language with beam search, if the requested attempts produce no text and detection
  succeeds with a different language
- detected language with greedy decoding, if the requested attempts produce no text and detection
  succeeds with a different language

If all attempts still produce no subtitle text, the terminal error includes per-attempt
segment diagnostics.

### Transcription engines

`whisper-rs` remains the zero-Python default and supports `tiny`, `base`, `small`, `medium`, and
their `.en` variants. It is the simpler installation and retains deterministic local adaptive-RMS
segmentation. For higher transcription quality, select faster-whisper with `large-v3-turbo` or
`large-v3`:

```bash
lilaccaps transcribe input.mp4 --engine faster-whisper --model large-v3-turbo --lang zh
```

The faster-whisper backend is a pinned PEP 723 Python helper managed entirely by `uv`; no pip or
project virtual-environment workflow is required. Its model is downloaded into
`$LILACCAPS_HOME/models/faster-whisper` on first use. It enables faster-whisper word timestamps and
Silero VAD, reports `segmentation_strategy = silero-vad`, and feeds the same deterministic cue
builder and QA path as `whisper-rs`. Selecting `--engine faster-whisper` without `--model`
automatically chooses `large-v3-turbo` when the configured model is incompatible.

| Engine | Models | Segmentation | Best fit |
| --- | --- | --- | --- |
| `whisper-rs` (default) | `tiny`, `base`, `small`, `medium`, `.en` variants | adaptive RMS, padded overlapping windows | simplest Rust-native local setup |
| `faster-whisper` | `large-v3-turbo`, `large-v3` | Silero VAD in faster-whisper | higher-quality Mandarin and long-form transcription |

The default `whisper-rs` path uses a deterministic 20 ms RMS analysis to derive an adaptive speech
threshold, bridge short pauses, remove isolated noise bursts, pad detected speech, and omit long
silent regions. Continuous speech is split into bounded 30-second windows with two seconds of
overlap. If no speech regions are detected, the command falls back to fixed overlapping windows.

The overlap gives Whisper context on both sides of long-speech boundaries. Each overlapping region
has one deterministic owner, and repeated or contained boundary cues are deduplicated before output,
so shared context does not produce double subtitles. Set `mode = "fixed"` for the Phase 1 behavior.
Successful `whisper-rs` output reports `segmentation_strategy` (`speech`, `fixed`, or
`fixed-fallback`) and `window_count`; stderr reports detected regions, threshold, and speech
coverage. The segmentation settings below apply to `whisper-rs`; faster-whisper owns its Silero VAD
pass.

Whisper token timestamps are the default cue source. UTF-8 token fragments are reassembled safely,
special tokens are ignored, and cues are rebuilt at punctuation and pauses with balanced,
language-aware character and duration limits. This is especially important for CJK text, where cue
boundaries cannot rely on spaces. Successful output reports `cue_timing = word`; if token timing or
reconstructed text fails integrity checks in any window, that window automatically uses the
segment-based proportional splitter and output reports `mixed` or `segment-fallback`.

Generated cues then pass through deterministic timing cleanup. Empty cues are removed, indexes are
renumbered, timestamps are clamped to the media duration, short cues are extended when space permits,
a small end padding is applied, and overlaps are eliminated. Structural SRT QA failures stop
publication; remaining readability findings are reported as
`transcribe_qa_warning` without discarding an otherwise valid transcript. Successful command output
includes `cue_count` and `qa_warning_count`.

For inputs longer than one minute, the command prints the extracted audio duration to stderr, and
multi-window runs print progress while preserving the normal stdout summary. This makes accidentally
long media, such as a downloaded file whose metadata says it is an hour long, visible instead of
looking like a post-audio-prep hang.

Default quality settings:

```toml
[transcribe]
engine = "whisper-rs"
language = "auto"

[transcribe.segmentation]
mode = "speech"
chunk_seconds = 30
overlap_seconds = 2
min_speech_ms = 400
min_silence_ms = 350
padding_ms = 300
max_window_seconds = 30

[transcribe.cues]
min_duration_ms = 800
max_duration_ms = 6000
end_padding_ms = 150
pause_split_ms = 500
max_chars_per_line = 42
max_cjk_chars_per_line = 18
max_lines = 2

[transcribe.cleanup]
enabled = false
command = "codex"
model = "gpt-5.6-terra"
```

To make the higher-quality engine persistent, set both engine and model:

```toml
[transcribe]
engine = "faster-whisper"

[transcribe.model]
id = "large-v3-turbo"
```

Omit `transcribe.model.path` to use the managed model cache. An explicit path is useful for an
already-downloaded local model directory.

### Optional conservative text cleanup

`--cleanup` enables a final text-only Codex pass after local transcription and timing optimization.
Use `--cleanup MODEL` to override the configured cleanup model for one run. The subprocess uses
`codex exec` in an isolated temporary working directory with a read-only sandbox, ephemeral session,
and a strict JSON schema. Validation requires every cue index exactly once, preserves all timestamps,
rejects empty/multiline text, and rejects edits that rewrite more than half of a cue. Any command,
schema, or validation failure aborts without publishing the output SRT.

Cleanup is disabled by default because subtitle text is sent to the configured Codex provider. It
does not send the source audio or video, but the transcript may still be sensitive. Keep it disabled
for fully local work or when the recognition text must not leave the machine. The local deterministic
QA pass always runs, regardless of cleanup. `transcribe.cleanup.command` may be an executable name
on `PATH` or an absolute path to a specific Codex binary; relative command paths are rejected.

`translate.model` defaults to `"gemini-3.1-flash-lite"`. The retired
`"gemini-3.1-flash-lite-preview"` managed default is migrated automatically during install or
update. `translate.append`
defaults to `true`, which means translated lines are appended below the original cue text.
`translate.default_targets` can provide default `--to` languages for `lilaccaps translate`.
`translate.line_order` controls top-to-bottom line order inside each multilingual cue.

Example:

```toml
[translate]
model = "gemini-3.1-flash-lite"
append = true
default_targets = ["en", "ja"]
line_order = ["source", "ja", "en"]
```

`lilaccaps translate` loads `GEMINI_API_KEY` from the environment first, then from
`$LILACCAPS_HOME/.env` (default `~/.lilac/lilaccaps/.env`). It keeps cue timing and
indexes unchanged and only rewrites cue text.

## Uninstall

Preview the owned paths, then confirm removal explicitly:

```bash
lilaccaps uninstall
lilaccaps uninstall --yes
```

Install creates a `.lilaccaps-runtime` ownership marker. Recursive uninstall refuses relative,
symlinked, shallow, protected, or unmarked custom runtime directories before it removes the binary, config,
or generated skill. The historical default `~/.lilac/lilaccaps` remains removable for compatibility
with installations created before the marker was introduced.

`burnin.font` defaults to `"auto"`, which lets the renderer choose a suitable font. The
ImageMagick fallback prefers a CJK-capable font for CJK subtitles and a lighter Latin font
otherwise. Set `burnin.font` to a system font name such as `"PingFang SC"` or `"Arial"` to
force a specific font for burn-in, or pass `--font` on the CLI for a single run.

`burnin.colour` defaults to `"auto"`, which keeps the renderer's default caption colour.
Set it to an ImageMagick colour string such as `"yellow"`, `"#ffd54f"`, or `"rgb(255,220,120)"`
to change subtitle fill colour, or pass `--colour` on the CLI for a single run. The CLI also
accepts `--color` as an alias. Explicit colour values are applied through the overlay renderer
so the configured fill colour is honored consistently. The overlay renderer preserves colour by
rendering outline, fill, and shadow as separate layers.

`burnin.size` defaults to `0`, which means auto-size from the video height. Set a positive
number in `lilaccaps.toml` or pass `--size` on the CLI to force a point size. CLI values
override TOML values for a single run.

`burnin.line_spacing` defaults to `0`, which means auto spacing. Set a positive number in
`lilaccaps.toml` to control the vertical gap between lines in multiline burn-in subtitles.
This setting is applied by the ImageMagick overlay renderer; when you set it explicitly,
`lilaccaps` prefers that renderer so the spacing value is honored.

The ImageMagick overlay renderer wraps long subtitle lines to a safe width inside the
video frame before centering them. Explicit line breaks already present in the `.srt`
are preserved, and long single lines are wrapped so the beginning and end do not run
outside the visible video area.

`burnin.outline.enabled` defaults to `true`, `burnin.outline.colour` defaults to `"black"`,
and `burnin.outline.width` defaults to `2`. This draws a readable border around burn-in
subtitle text for videos where the background changes from dark to light. Set `width = 0`
or `enabled = false` to disable it, or pass `--no-outline` for a single run. You can also
override one run with `--outline-colour black --outline-width 3`; `--outline-color` is an
alias. The default black outline is compatible with the primary `ffmpeg` subtitle renderer;
unusual outline colour strings may use the ImageMagick overlay fallback so the colour is honored.

`burnin.advanced_styling` defaults to `true`. Set it to `false` to ignore configured custom
colour, custom line spacing, and all `burnin.styles.<role>` per-line styling so `lilaccaps`
can stay on the primary `ffmpeg` subtitle path when your `ffmpeg` build supports it. An
explicit CLI `--colour` still overrides config for that single run.

For per-language burn-in styling, define keyed styles under `burnin.styles` using the same
labels that appear in `translate.line_order`, such as `source`, `en`, or `ja`. Set
`font = "auto"` or omit `font` to keep renderer font selection per line. Set
`colour = "auto"` or omit `colour` to keep the renderer's default colour per line.

Example:

```toml
[translate]
append = true
default_targets = ["en", "ja"]
line_order = ["source", "ja", "en"]

[burnin]
# Keep this `true` for per-line styles, custom colours, and custom spacing.
# Set it to `false` to ignore those advanced settings and prefer ffmpeg/libass.
advanced_styling = true
font = "auto"
colour = "auto"
size = 0
line_spacing = 1

[burnin.outline]
enabled = true
colour = "black"
width = 2

[burnin.styles.source]
font = "auto"
colour = "auto"
size = 42

[burnin.styles.ja]
font = "auto"
colour = "auto"
size = 38

[burnin.styles.en]
font = "auto"
colour = "auto"
size = 34
```

With that config, the first line in each cue uses the `source` style, the second uses `ja`,
and the third uses `en`.

## Repository Layout

```text
src/
  commands/
  pipelines/
  media.rs
  model.rs
  render.rs
  subtitles.rs
```

Supporting planning docs:

- [lilaccaps-plan.md](./lilaccaps-plan.md)
- [handoff.md](./handoff.md)
- [video-subtitle-pipeline-proposal.md](./video-subtitle-pipeline-proposal.md)

## Status

This project is usable now for:

- local transcription to `.srt`
- subtitle translation with preserved cue timing
- burn-in rendering from existing `.srt`
- text and image watermark rendering
- installation and environment health reporting

The current implementation favors clean boundaries over broad convenience wrappers.

## License

MIT. See [LICENSE](./LICENSE).
