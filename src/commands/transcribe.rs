use anyhow::Result;

use crate::cli::TranscribeArgs;
use crate::pipelines;

pub fn run(args: TranscribeArgs) -> Result<()> {
    let output = pipelines::transcribe::run(args.input, args.config_path, args.output, args.lang)?;
    println!("command = transcribe");
    println!("input = {}", output.input.display());
    println!("output = {}", output.output.display());
    println!("model_path = {}", output.model_path.display());
    println!("language = {}", output.language);
    println!("decoding_strategy = {}", output.decoding_strategy);
    println!("fallback_language_used = {}", output.fallback_language_used);
    println!("fallback_decoding_used = {}", output.fallback_decoding_used);
    println!("status = {}", output.status);
    Ok(())
}
