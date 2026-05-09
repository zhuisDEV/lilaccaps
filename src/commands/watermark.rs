use anyhow::Result;

use crate::cli::WatermarkArgs;
use crate::pipelines;

pub fn run(args: WatermarkArgs) -> Result<()> {
    let output = pipelines::watermark::run(
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
    )?;
    println!("command = watermark");
    println!("video = {}", output.video.display());
    println!("output = {}", output.output.display());
    println!("watermark = {}", output.watermark);
    println!("position = {}", output.position);
    println!("opacity = {:.2}", output.opacity);
    println!("size = {}", output.size);
    println!("margin = {}", output.margin);
    println!("status = {}", output.status);
    Ok(())
}
