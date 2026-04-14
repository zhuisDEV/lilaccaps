use anyhow::Result;

use crate::cli::BurninArgs;
use crate::pipelines;

pub fn run(args: BurninArgs) -> Result<()> {
    let output = pipelines::burnin::run(args.video, args.config_path, args.subs, args.output)?;
    println!("command = burnin");
    println!("video = {}", output.video.display());
    println!("subs = {}", output.subs.display());
    println!("output = {}", output.output.display());
    println!("status = {}", output.status);
    Ok(())
}
