use anyhow::Result;

use crate::cli::StatusArgs;
use crate::config::load_config;
use crate::integration::detect_skill_path;
use crate::model::resolved_model_path;
use crate::release::latest_release;
use crate::runtime::{
    collect_doctor_report, current_executable, detect_runtime_health, install_binary_path,
};

pub fn run(args: StatusArgs) -> Result<()> {
    let loaded = load_config(args.config_path)?;
    let release = latest_release(loaded.config.release.github_repo.as_deref())
        .ok()
        .flatten();
    let install_path = install_binary_path()?;
    let current_exe = current_executable()?;
    let runtime_health = detect_runtime_health(&loaded.paths, &loaded.config);
    let doctor_report = collect_doctor_report();
    let detected_skill = detect_skill_path(&loaded.config.agent);
    let model_path = resolved_model_path(&loaded.paths, &loaded.config)?;
    let detected_skill_string = detected_skill
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());

    if args.json {
        println!("{{");
        println!("  \"version\": \"{}\",", env!("CARGO_PKG_VERSION"));
        println!(
            "  \"latest_stable\": {},",
            json_string(release.as_ref().map(|item| item.version.as_str()))
        );
        println!(
            "  \"binary_path\": {},",
            json_string(Some(current_exe.to_string_lossy().as_ref()))
        );
        println!(
            "  \"installed_binary_path\": {},",
            json_string(Some(install_path.to_string_lossy().as_ref()))
        );
        println!(
            "  \"config_path\": {},",
            json_string(Some(loaded.paths.config_path.to_string_lossy().as_ref()))
        );
        println!(
            "  \"runtime_home\": {},",
            json_string(Some(loaded.paths.runtime_home.to_string_lossy().as_ref()))
        );
        println!(
            "  \"skill_path\": {},",
            json_string(Some(
                loaded.config.agent.skill_path.to_string_lossy().as_ref()
            ))
        );
        println!(
            "  \"skill_detected\": {},",
            json_string(detected_skill_string.as_deref())
        );
        println!(
            "  \"model_path\": {},",
            json_string(Some(model_path.to_string_lossy().as_ref()))
        );
        println!("  \"config_valid\": {},", runtime_health.config_valid);
        println!("  \"installed\": {},", runtime_health.installed);
        println!("  \"healthy\": {},", runtime_health.healthy);
        println!("  \"cargo_available\": {},", runtime_health.cargo_available);
        println!(
            "  \"ffmpeg_available\": {},",
            runtime_health.ffmpeg_available
        );
        println!(
            "  \"ffprobe_available\": {},",
            runtime_health.ffprobe_available
        );
        println!("  \"cmake_available\": {},", runtime_health.cmake_available);
        println!(
            "  \"magick_available\": {},",
            runtime_health.magick_available
        );
        println!("  \"build_ready\": {},", runtime_health.build_ready);
        println!(
            "  \"fallback_renderer_ready\": {},",
            runtime_health.fallback_renderer_ready
        );
        println!(
            "  \"can_fix_with_brew\": {},",
            doctor_report.can_fix_with_brew
        );
        println!("  \"model_ready\": {},", runtime_health.model_ready);
        println!("  \"missing\": [");
        for (index, item) in runtime_health.missing.iter().enumerate() {
            let suffix = if index + 1 == runtime_health.missing.len() {
                ""
            } else {
                ","
            };
            println!("    \"{}\"{}", item, suffix);
        }
        println!("  ],");
        println!("  \"brew_packages\": [");
        for (index, item) in doctor_report.brew_packages.iter().enumerate() {
            let suffix = if index + 1 == doctor_report.brew_packages.len() {
                ""
            } else {
                ","
            };
            println!("    \"{}\"{}", item, suffix);
        }
        println!("  ],");
        println!("  \"advisories\": [");
        for (index, item) in doctor_report.advisories.iter().enumerate() {
            let suffix = if index + 1 == doctor_report.advisories.len() {
                ""
            } else {
                ","
            };
            println!("    \"{}\"{}", item, suffix);
        }
        println!("  ]");
        println!("}}");
        return Ok(());
    }

    println!("version = {}", env!("CARGO_PKG_VERSION"));
    println!(
        "latest_stable = {}",
        release
            .as_ref()
            .map(|item| item.version.as_str())
            .unwrap_or("unavailable")
    );
    println!("binary_path = {}", current_exe.display());
    println!("installed_binary_path = {}", install_path.display());
    println!("config_path = {}", loaded.paths.config_path.display());
    println!("runtime_home = {}", loaded.paths.runtime_home.display());
    println!("skill_path = {}", loaded.config.agent.skill_path.display());
    println!(
        "skill_detected = {}",
        detected_skill_string.unwrap_or_else(|| "unavailable".to_string())
    );
    println!("model_path = {}", model_path.display());
    println!("config_valid = {}", runtime_health.config_valid);
    println!("installed = {}", runtime_health.installed);
    println!("healthy = {}", runtime_health.healthy);
    println!("cargo_available = {}", runtime_health.cargo_available);
    println!("ffmpeg_available = {}", runtime_health.ffmpeg_available);
    println!("ffprobe_available = {}", runtime_health.ffprobe_available);
    println!("cmake_available = {}", runtime_health.cmake_available);
    println!("magick_available = {}", runtime_health.magick_available);
    println!("build_ready = {}", runtime_health.build_ready);
    println!(
        "fallback_renderer_ready = {}",
        runtime_health.fallback_renderer_ready
    );
    println!("can_fix_with_brew = {}", doctor_report.can_fix_with_brew);
    println!("model_ready = {}", runtime_health.model_ready);
    println!(
        "missing = {}",
        if runtime_health.missing.is_empty() {
            "none".to_string()
        } else {
            runtime_health.missing.join(", ")
        }
    );
    println!(
        "brew_packages = {}",
        if doctor_report.brew_packages.is_empty() {
            "none".to_string()
        } else {
            doctor_report.brew_packages.join(", ")
        }
    );
    println!(
        "advisories = {}",
        if doctor_report.advisories.is_empty() {
            "none".to_string()
        } else {
            doctor_report.advisories.join(", ")
        }
    );

    Ok(())
}

fn json_string(value: Option<&str>) -> String {
    value
        .map(|item| format!("\"{}\"", item.replace('\\', "\\\\").replace('"', "\\\"")))
        .unwrap_or_else(|| "null".to_string())
}
