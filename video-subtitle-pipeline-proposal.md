# Proposal: Add a Separate Video Subtitle Pipeline Skill for OpenClaw

## Implementation Status (2026-08-08)

This document is retained as the original architectural proposal. The dedicated implementation is
now `lilaccaps`, with the repo-root `SKILL.md` as its OpenClaw quick start and `README.md` as the
current manual.

Implemented from this proposal:

- local media to `.srt` transcription
- selectable local `whisper-rs` and uv-managed faster-whisper engines
- speech-aware/Silero VAD segmentation, word-timestamp cue rebuilding, timing optimization, and SRT
  QA
- optional conservative Codex text cleanup with unchanged cue structure and timing
- subtitle translation and multilingual cue composition
- burned-in subtitle video output
- separate lifecycle, health, configuration, and OpenClaw integration boundaries

Not currently implemented:

- plain `.txt` transcripts or `.vtt`
- selectable soft-subtitle muxing
- speaker segmentation
- automatic `media-ingest` download orchestration

Those remain possible independent additions, so this original roadmap is not a finished plan file.
They must compose with the existing commands rather than move acquisition, transcription,
translation, and rendering into one hidden workflow.

## Conclusion
Create a **new standalone skill** for subtitle generation and subtitle-enabled video output, instead of expanding `media-ingest` itself. Keep `media-ingest` focused on media acquisition, and let the new skill orchestrate transcription, `.srt` generation, translation, and subtitle muxing/burn-in through a proper subagent workflow.

## Recommendation
Introduce a new skill, for example:
- `video-subtitle-pipeline`
- or `video-transcribe-subtitle`

This skill should:
1. Accept a local video file path or a downloaded artifact path.
2. Spawn a proper coding/subtask agent when multi-step work is needed.
3. Extract or normalize audio when necessary.
4. Run speech-to-text transcription.
5. Produce timestamped subtitle files such as `.srt`.
6. Optionally translate subtitles into another language.
7. Output either:
   - subtitle file only,
   - soft-subtitled video,
   - burned-in subtitle video,
   - or both.

## Why a Separate Skill Is Better

### 1. Separation of concerns
`media-ingest` should stay responsible for:
- fetching media,
- downloading artifacts,
- saving outputs.

Subtitle generation is a different pipeline involving:
- ASR,
- timing cleanup,
- translation,
- subtitle formatting,
- video remuxing or re-encoding.

Keeping them separate reduces complexity and preserves clarity.

### 2. Easier maintenance
A dedicated subtitle skill makes it easier to:
- swap transcription engines,
- improve subtitle cleanup,
- support translation,
- add style presets,
- support different output modes.

### 3. Better subagent fit
Subtitle workflows are multi-step and can require:
- command selection,
- retries,
- formatting validation,
- output verification,
- language-specific handling.

That is a good fit for a spawned subagent instead of a single monolithic ingestion tool.

### 4. Cleaner expansion path
This design supports future modes without bloating `media-ingest`, such as:
- `srt-only`
- `softsub`
- `burned-in`
- `translate-to-zh`
- `translate-to-en`
- `bilingual-subtitles`
- `speaker-segmented-subtitles`

## Proposed Architecture

### Layer 1: Media acquisition
Existing tool:
- `media-ingest`

Responsibility:
- download and save the media asset.

### Layer 2: Subtitle pipeline skill
New skill:
- `video-subtitle-pipeline`

Responsibility:
- accept source video,
- decide workflow,
- spawn subagent when appropriate,
- create `.srt`,
- create optional subtitled outputs.

### Layer 3: Optional wrapper workflow
Later, a convenience wrapper can combine both steps:
- download video,
- generate subtitles,
- export final subtitled asset.

This should be a composition layer, not a redefinition of `media-ingest`.

## Proposed User-Facing Capabilities
The new skill should support prompts like:
- "Generate subtitles for this video."
- "Create an `.srt` from this file."
- "Burn English subtitles into this MP4."
- "Translate the subtitles to Chinese and output a new video."
- "Make a soft-subtitled version for Apple Photos or VLC."
- "Download this video, transcribe it, and save the `.srt`."

## Proposed Output Modes

### Mode A: Transcript only
Outputs:
- `.txt`
- optional cleaned transcript markdown

### Mode B: Subtitle file only
Outputs:
- `.srt`
- optional `.vtt`

### Mode C: Soft subtitles
Outputs:
- original or remuxed video with selectable subtitle track

### Mode D: Burned-in subtitles
Outputs:
- re-encoded video with visible subtitles baked into the image

### Mode E: Translation pipeline
Outputs:
- translated `.srt`
- optional bilingual subtitle version
- optional translated burned-in video

## Proposed Skill Responsibilities
The skill should:
1. Validate input file path.
2. Probe media streams.
3. Decide whether audio extraction is needed.
4. Generate transcript with timestamps.
5. Normalize subtitle timing and line breaks.
6. Validate `.srt` formatting.
7. Create final requested outputs.
8. Save results into workspace-approved folders.
9. Report exact output paths.

## Proposed Subagent Role
The subagent should handle the complex work, including:
- selecting the best transcription flow,
- handling long videos,
- choosing whether to segment audio,
- verifying subtitle sync,
- producing the final deliverables.

Main session responsibility should remain lightweight:
- gather intent,
- call the skill,
- receive final paths,
- report results.

## Suggested Skill Interface
Example requests:
- `Generate SRT for /path/to/video.mp4`
- `Add burned subtitles to /path/to/video.mp4`
- `Translate subtitles to Chinese and burn them into the video`
- `Create soft subtitles for Apple-friendly playback`

Optional flags or parameters the skill may support:
- output mode
- subtitle language
- translation target
- burn vs softsub
- bilingual on/off
- output directory

## Implementation Strategy

### Phase 1: Minimal viable pipeline
Deliver:
- local video input
- transcription
- `.srt` generation
- exact output path reporting

### Phase 2: Video subtitle output
Add:
- soft subtitle muxing
- burned subtitle rendering

### Phase 3: Translation and bilingual support
Add:
- subtitle translation
- bilingual line formatting
- style presets

### Phase 4: Wrapper convenience flow
Add:
- one-command workflow that chains `media-ingest` and subtitle generation

## Risk Notes

### Do not overload `media-ingest`
If subtitle logic is packed directly into `media-ingest`, it will:
- blur responsibility boundaries,
- increase maintenance burden,
- make failure handling harder,
- complicate future evolution.

### Prefer composition
The right model is:
- keep acquisition simple,
- add subtitle intelligence as a separate layer,
- compose them when needed.

## Final Recommendation
Proceed with a **new dedicated subtitle skill** that orchestrates transcription and subtitle/video output through a spawned subagent. Keep `media-ingest` unchanged as the acquisition layer, and build the subtitle capability as a composable extension above it.

This is the cleaner, safer, and more extensible architecture.
