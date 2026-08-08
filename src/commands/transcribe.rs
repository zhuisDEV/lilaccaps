use anyhow::Result;

use crate::cli::TranscribeArgs;
use crate::pipelines;

pub fn run(args: TranscribeArgs) -> Result<()> {
    let output = pipelines::transcribe::run(
        args.input,
        args.config_path,
        args.output,
        args.lang,
        args.engine,
        args.model,
        args.cleanup,
    )?;
    println!("command = transcribe");
    println!("input = {}", output.input.display());
    println!("output = {}", output.output.display());
    println!("model_path = {}", output.model_path.display());
    println!("engine = {}", output.engine);
    println!("model = {}", output.model);
    println!("language = {}", output.language);
    println!("decoding_strategy = {}", output.decoding_strategy);
    println!("fallback_language_used = {}", output.fallback_language_used);
    println!("fallback_decoding_used = {}", output.fallback_decoding_used);
    println!("cue_count = {}", output.cue_count);
    println!("qa_warning_count = {}", output.qa_warning_count);
    println!("cue_timing = {}", output.cue_timing);
    println!("segmentation_strategy = {}", output.segmentation_strategy);
    println!("window_count = {}", output.window_count);
    println!("cleanup = {}", output.cleanup);
    println!("status = {}", output.status);
    Ok(())
}
