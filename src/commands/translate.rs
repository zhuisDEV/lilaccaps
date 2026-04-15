use anyhow::Result;

use crate::cli::TranslateArgs;
use crate::pipelines;

pub fn run(args: TranslateArgs) -> Result<()> {
    let output = pipelines::translate::run(
        args.input,
        args.config_path,
        args.output,
        args.to,
        args.append,
    )?;
    println!("command = translate");
    println!("input = {}", output.input.display());
    println!("output = {}", output.output.display());
    println!("targets = {}", output.targets.join(","));
    println!("append = {}", output.append);
    println!("model = {}", output.model);
    println!("status = {}", output.status);
    Ok(())
}
