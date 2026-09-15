use anyhow::Result;

use crate::cli::WatermarkArgs;
use crate::config::load_config;
use crate::pipelines;

pub fn run(args: WatermarkArgs) -> Result<()> {
    let output = if let Some(preset) = args.preset {
        let loaded = load_config(args.config_path)?;
        pipelines::watermark::run_saved(
            args.video,
            args.output,
            &loaded.paths.runtime_home,
            &preset,
        )?
    } else {
        pipelines::watermark::run(
            args.video,
            args.output,
            args.text,
            args.image,
            args.position,
            args.opacity,
            args.size,
            args.margin,
            args.colour,
            args.font,
            args.outline_colour,
            args.outline_width,
        )?
    };
    println!("command = watermark");
    println!("video = {}", output.video.display());
    println!("output = {}", output.output.display());
    println!("watermark = {}", output.watermark);
    println!("position = {}", output.position);
    println!("opacity = {:.2}", output.opacity);
    println!("size = {}", output.size);
    println!("margin = {}", output.margin);
    println!("renderer = {}", output.renderer);
    println!("renderer_reason = {}", output.renderer_reason);
    println!("status = {}", output.status);
    Ok(())
}
