use anyhow::Result;

use crate::cli::BurninArgs;
use crate::pipelines;

pub fn run(args: BurninArgs) -> Result<()> {
    let output = pipelines::burnin::run(
        args.video,
        args.config_path,
        args.subs,
        args.output,
        args.font,
        args.colour,
        args.size,
    )?;
    println!("command = burnin");
    println!("video = {}", output.video.display());
    println!("subs = {}", output.subs.display());
    println!("output = {}", output.output.display());
    println!("font = {}", output.font);
    println!("colour = {}", output.colour);
    println!("size = {}", output.size);
    println!("line_spacing = {}", output.line_spacing);
    println!("renderer = {}", output.renderer);
    println!("renderer_reason = {}", output.renderer_reason);
    println!("status = {}", output.status);
    Ok(())
}
