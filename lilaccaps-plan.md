# lilaccaps Control Plan

## Purpose
`lilaccaps` is the canonical CLI and project name for a clean subtitle pipeline focused on two distinct responsibilities:

1. subtitle generation
2. subtitle rendering

The primary flow must keep these responsibilities separate. Fallback behavior must not be mixed into the primary path.

## Product Decisions
- Canonical name: `lilaccaps`
- Unified CLI entrypoint: `lilaccaps`
- CLI lifecycle commands include `install`, `update`, `status`, and `uninstall`
- `lilaccaps burnin` is render-only
- Subtitle generation is a separate command/workflow
- Video/audio to captions may be delegated by the main OpenClaw agent to a subagent
- Use Rust/TS/Deno
- Keep code and design clean and tidy

## Runtime And Configuration

### Config File
- Config file name: `lilaccaps.toml`

Primary config responsibilities:
- define the runtime home directory
- define agent integration paths
- persist explicit user overrides

### Runtime Home
- environment variable name: `LILACCAPS_HOME`
- runtime home concept in docs may also be described as `$lilaccaps_home`
- default directory: `~/.lilac/lilaccaps`

The runtime home should store:
- runtime state
- logs if needed
- cached artifacts if needed
- integration/bootstrap files if needed

### Agent Skill Path
The config should include an agent skill path.

OpenClaw default target:
- `~/.openclaw/skills/lilaccaps/SKILL.md`

Preferred behavior:
1. detect an existing compatible agent skill directory automatically
2. if detection is inconclusive, fall back to explicit config
3. if setup still remains ambiguous, provide a generated bootstrap document with exact instructions

### Bootstrap Guidance
If autodetection is not reliable enough in the first implementation, ship a simple `bootstrap.md` flow that tells the agent/operator how to:
- set `LILACCAPS_HOME`
- place or link the skill file
- confirm the integration path
- verify status

## Primary Flow

### Flow A: Generate Captions
Input:
- local video file
- local audio file

Output:
- subtitle artifact such as `.srt`
- optional transcript artifact if explicitly supported

Responsibility:
- probe input
- extract or normalize audio if needed
- run transcription
- normalize subtitle timing and line wrapping
- validate subtitle format
- write exact output paths

This flow should be exposed as a dedicated command, for example:
- `lilaccaps captions <input>`
- or `lilaccaps transcribe <input>`

Recommendation:
- use one command name consistently across the project, docs, and internal modules
- prefer `captions` if the output contract is subtitle-first rather than raw transcript-first

### Flow B: Burn In Existing Captions
Input:
- local video file
- existing subtitle or caption file

Output:
- rendered video with burned-in subtitles

Responsibility:
- validate video input
- validate subtitle input
- render subtitles into video
- write exact output path

Command:
- `lilaccaps burnin <video> --subs <subtitle-file>`

This command must not trigger subtitle generation internally.

## Command Model
Proposed top-level shape:

```text
lilaccaps install [options]
lilaccaps update [options]
lilaccaps status [options]
lilaccaps uninstall [options]
lilaccaps captions <input> [options]
lilaccaps burnin <video> --subs <subtitle-file> [options]
```

Optional later expansion:

```text
lilaccaps softsub <video> --subs <subtitle-file> [options]
lilaccaps translate <subtitle-file> [options]
```

The initial implementation should focus on:
1. `install`
2. `status`
3. `captions`
4. `burnin`

## Architecture

### 1. CLI Layer
Responsibility:
- parse commands and arguments
- validate required inputs
- dispatch to one workflow only
- report outputs clearly

This layer also owns:
- config discovery
- release/version inspection
- environment validation
- uninstall safety checks

Suggested implementation:
- Rust CLI as the primary executable
- TS/Deno allowed for helper tooling or agent-facing orchestration where it is a better fit

### 2. Caption Generation Pipeline
Responsibility:
- media probe
- optional audio extraction
- transcription
- subtitle normalization
- `.srt` serialization

This pipeline should be isolated from rendering concerns.

### 3. Burn-In Rendering Pipeline
Responsibility:
- subtitle file validation
- render configuration
- video re-encode with burned-in captions

This pipeline should assume captions already exist.

### 4. Agent Orchestration Boundary
Responsibility:
- allow the main OpenClaw agent to delegate video/audio to captions work to a subagent when appropriate
- keep the main agent focused on intent gathering and result reporting

Delegation applies to caption generation, not to redefining the `burnin` command.

### 5. Install And Lifecycle Management
Responsibility:
- install the CLI globally
- check for newer stable releases from GitHub
- report environment health
- remove installed assets cleanly

Lifecycle commands should remain separate from media workflows.

## Lifecycle Commands

### `lilaccaps install`
Purpose:
- install the CLI globally

Expected behavior:
- install or place the binary in the usual Cargo location, typically `~/.cargo/bin`
- initialize `lilaccaps.toml` if missing
- initialize runtime home if missing
- set up or guide agent skill integration
- report final binary and config locations

Primary path:
- prefer a clean Rust-native install flow first

Fallback:
- if full automation is not reliable across environments, generate or print exact bootstrap steps separately

### `lilaccaps update`
Purpose:
- update the installed CLI to the latest stable release from the GitHub repository

Expected behavior:
- detect current installed version
- detect latest stable release
- upgrade deterministically
- preserve config and runtime state unless migration is explicitly needed

This command should treat release discovery and upgrade as one lifecycle workflow, separate from subtitle features.

### `lilaccaps status`
Purpose:
- report important runtime and installation information

Expected output should include:
- current installed version
- latest available stable release
- binary path
- config file path
- runtime home path
- agent skill path
- whether config is valid
- whether required dependencies are missing
- whether the installation is healthy

Status should be a high-signal health report, not a verbose dump.

### `lilaccaps uninstall`
Purpose:
- remove `lilaccaps` from disk completely

Expected behavior:
- remove installed binary
- remove config file if owned by `lilaccaps`
- remove runtime home
- remove generated integration files if owned by `lilaccaps`
- clearly warn before destructive deletion

This command should be explicit and conservative about deleting user-managed paths.

## Primary Implementation Order

### Phase 1
Deliver:
- `lilaccaps` CLI skeleton
- config discovery and `lilaccaps.toml`
- runtime home resolution
- `install` command contract
- `status` command contract
- `captions` command contract
- `burnin` command contract
- shared input/output path handling

### Phase 2
Deliver:
- install bootstrap path
- version inspection and release-check plumbing
- environment validation in `status`
- caption generation pipeline to `.srt`
- exact output path reporting
- deterministic error handling

### Phase 3
Deliver:
- `update` workflow
- `uninstall` workflow
- `burnin` rendering pipeline
- render options that remain narrow and explicit

### Phase 4
Deliver:
- agent delegation support for caption generation
- optional translation or softsub as separate workflows

## Fallback Strategy
Fallback is a separate concern and should be implemented after the primary flow is stable.

Possible fallback areas:
- alternate transcription backend
- alternate subtitle normalization path
- alternate render backend

Rules:
- do not hide fallback selection inside the main happy path
- do not mix primary and fallback logic in the same command branch
- keep backend selection explicit in configuration or internal adapters

## Clean Design Rules
- Keep lifecycle commands separate from subtitle-processing commands
- Keep `captions` and `burnin` as separate workflows with separate modules
- Do not let `burnin` call transcription internally
- Prefer explicit artifacts over implicit side effects
- Keep command semantics narrow
- Report exact output paths
- Keep primary path simple before adding fallback adapters
- Favor small, composable modules over one large pipeline

## Suggested Repository Shape
Example only:

```text
src/
  cli/
  commands/
    install.rs
    update.rs
    status.rs
    uninstall.rs
    captions.rs
    burnin.rs
  config/
  runtime/
  integration/
  release/
  pipelines/
    captions/
    burnin/
  media/
  subtitles/
  render/
  errors/
```

If TS/Deno is introduced, keep it behind a clean boundary, for example:

```text
tools/
  agent/
  transcription/
```

## Output Contract
Every successful command should report:
- input path used
- output path produced
- output type produced

Examples:
- initialized `lilaccaps.toml`
- installed binary in `~/.cargo/bin`
- generated `.srt`
- burned-in `.mp4`

## Non-Goals For Initial Delivery
- combining generation and burn-in into one hidden workflow
- overloading one command with multiple responsibilities
- mixing fallback engines into the primary control path
- broad convenience wrappers before the core flows are reliable

## Immediate Next Step
Build the `lilaccaps` CLI skeleton around first-class commands:
- `install`
- `status`
- `captions`
- `burnin`

Then implement `.srt` generation first, followed by render-only burn-in, with `update` and `uninstall` added as separate lifecycle workflows.
