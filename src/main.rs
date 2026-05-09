mod cli;
mod commands;
mod config;
mod integration;
mod media;
mod model;
mod pipelines;
mod release;
mod render;
mod runtime;
mod subtitles;
mod translate;
mod watermark;

use anyhow::Result;

fn main() -> Result<()> {
    cli::run()
}
