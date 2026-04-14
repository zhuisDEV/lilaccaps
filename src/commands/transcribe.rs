use anyhow::Result;

use crate::cli::TranscribeArgs;
use crate::pipelines;

pub fn run(args: TranscribeArgs) -> Result<()> {
    let output = pipelines::transcribe::run(args.input, args.config_path, args.output)?;
    println!("command = transcribe");
    println!("input = {}", output.input.display());
    println!("output = {}", output.output.display());
    println!("model_path = {}", output.model_path.display());
    println!("status = {}", output.status);
    Ok(())
}
