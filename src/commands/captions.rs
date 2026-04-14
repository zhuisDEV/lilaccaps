use anyhow::Result;

use crate::cli::CaptionsArgs;
use crate::pipelines;

pub fn run(args: CaptionsArgs) -> Result<()> {
    let output = pipelines::captions::run(args.input, args.config_path, args.output)?;
    println!("command = captions");
    println!("input = {}", output.input.display());
    println!("output = {}", output.output.display());
    println!("model_path = {}", output.model_path.display());
    println!("status = {}", output.status);
    Ok(())
}
