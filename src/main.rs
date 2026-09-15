mod caption_agent;
mod cleanup;
mod cli;
mod commands;
mod config;
mod faster_whisper;
mod fonts;
mod integration;
mod media;
mod model;
mod pipelines;
mod release;
mod render;
mod runtime;
mod segmentation;
mod subtitles;
mod translate;
mod verification;
mod watermark;
mod watermark_presets;
mod workflow;

use anyhow::Result;

fn main() -> Result<()> {
    cli::run()
}
