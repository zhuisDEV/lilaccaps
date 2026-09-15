use anyhow::Result;

use crate::cli::{WatermarkPresetArgs, WatermarkPresetCommand};
use crate::config::load_config;
use crate::pipelines::watermark::resolve_source;
use crate::watermark::WatermarkStyle;
use crate::watermark_presets::{self, SavedWatermark};

pub fn run(args: WatermarkPresetArgs) -> Result<()> {
    match args.command {
        WatermarkPresetCommand::Save(args) => {
            let loaded = load_config(args.config_path)?;
            let source = resolve_source(args.text, args.image)?;
            let style = WatermarkStyle {
                position: args.position,
                opacity: args.opacity,
                size: args.size,
                margin: args.margin,
                colour: args.colour,
                font: args.font,
                outline_colour: args.outline_colour,
                outline_width: args.outline_width,
            };
            let saved =
                watermark_presets::save(&loaded.paths.runtime_home, &args.name, &source, &style)?;
            println!("command = watermark-preset save");
            print_preset(&saved);
            println!("status = saved");
        }
        WatermarkPresetCommand::List(args) => {
            let loaded = load_config(args.config_path)?;
            let presets = watermark_presets::list(&loaded.paths.runtime_home)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&presets)?);
            } else {
                println!("command = watermark-preset list");
                println!("count = {}", presets.len());
                for preset in presets {
                    println!(
                        "{}\t{}\t{}",
                        preset.name,
                        preset.style.position.label(),
                        preset.source.label()
                    );
                }
            }
        }
        WatermarkPresetCommand::Show(args) => {
            let loaded = load_config(args.config_path)?;
            let preset = watermark_presets::load(&loaded.paths.runtime_home, &args.name)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&preset)?);
            } else {
                println!("command = watermark-preset show");
                print_preset(&preset);
            }
        }
        WatermarkPresetCommand::Remove(args) => {
            let loaded = load_config(args.config_path)?;
            watermark_presets::remove(&loaded.paths.runtime_home, &args.name)?;
            println!("command = watermark-preset remove");
            println!("name = {}", args.name);
            println!("status = removed");
        }
    }
    Ok(())
}

fn print_preset(preset: &SavedWatermark) {
    println!("name = {}", preset.name);
    println!("format_version = {}", preset.format_version);
    println!("directory = {}", preset.directory.display());
    println!("watermark = {}", preset.source.label());
    println!("position = {}", preset.style.position.label());
    println!("opacity = {:.2}", preset.style.opacity);
    println!("size = {}", preset.style.size);
    println!("margin = {}", preset.style.margin);
    println!("colour = {}", preset.style.colour);
    println!("font = {}", preset.style.font.as_deref().unwrap_or("auto"));
    println!("outline_colour = {}", preset.style.outline_colour);
    println!("outline_width = {}", preset.style.outline_width);
}
