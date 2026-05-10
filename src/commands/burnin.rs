use anyhow::Result;

use crate::cli::BurninArgs;
use crate::pipelines::burnin::{self, BurninRequest};

pub fn run(args: BurninArgs) -> Result<()> {
    let output = burnin::run(BurninRequest {
        video: args.video,
        config_path: args.config_path,
        subs: args.subs,
        output: args.output,
        font: args.font,
        colour: args.colour,
        size: args.size,
        outline_enabled: if args.no_outline {
            Some(false)
        } else if args.outline {
            Some(true)
        } else {
            None
        },
        outline_colour: args.outline_colour,
        outline_width: args.outline_width,
    })?;
    println!("command = burnin");
    println!("video = {}", output.video.display());
    println!("subs = {}", output.subs.display());
    println!("output = {}", output.output.display());
    println!("font = {}", output.font);
    println!("colour = {}", output.colour);
    println!("size = {}", output.size);
    println!("line_spacing = {}", output.line_spacing);
    println!("outline_enabled = {}", output.outline_enabled);
    println!("outline_colour = {}", output.outline_colour);
    println!("outline_width = {}", output.outline_width);
    println!("renderer = {}", output.renderer);
    println!("renderer_reason = {}", output.renderer_reason);
    println!("status = {}", output.status);
    Ok(())
}
