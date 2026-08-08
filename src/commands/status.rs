use anyhow::Result;
use serde_json::{Map, Value, json};

use crate::cli::StatusArgs;
use crate::config::load_config;
use crate::integration::detect_skill_path;
use crate::model::resolved_model_path;
use crate::release::latest_release;
use crate::runtime::{
    DependencyStatus, collect_doctor_report_for_config, current_executable, detect_runtime_health,
    install_binary_path,
};

pub fn run(args: StatusArgs) -> Result<()> {
    let loaded = load_config(args.config_path)?;
    let (release, release_error) =
        match latest_release(loaded.config.release.github_repo.as_deref()) {
            Ok(release) => (release, None),
            Err(error) => (None, Some(error.to_string())),
        };
    let install_path = install_binary_path()?;
    let current_exe = current_executable()?;
    let runtime_health = detect_runtime_health(&loaded.paths, &loaded.config);
    let doctor_report = collect_doctor_report_for_config(&loaded.config);
    let detected_skill = detect_skill_path(&loaded.config.agent);
    let model_path = resolved_model_path(&loaded.paths, &loaded.config)?;
    let detected_skill_string = detected_skill
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());

    if args.json {
        let dependencies = doctor_report
            .statuses
            .iter()
            .map(|status| {
                (
                    status.dependency.name.to_string(),
                    json!({
                        "status": dependency_state(status),
                        "path": status.path.as_ref().map(|path| path.display().to_string()),
                        "version": status.version,
                        "error": status.error,
                    }),
                )
            })
            .collect::<Map<String, Value>>();
        let output = json!({
            "version": env!("CARGO_PKG_VERSION"),
            "latest_stable": release.as_ref().map(|item| item.version.as_str()),
            "latest_stable_error": release_error,
            "binary_path": current_exe.display().to_string(),
            "installed_binary_path": install_path.display().to_string(),
            "config_path": loaded.paths.config_path.display().to_string(),
            "runtime_home": loaded.paths.runtime_home.display().to_string(),
            "skill_path": loaded.config.agent.skill_path.display().to_string(),
            "skill_detected": detected_skill_string,
            "model_path": model_path.display().to_string(),
            "config_valid": runtime_health.config_valid,
            "installed": runtime_health.installed,
            "healthy": runtime_health.healthy,
            "cargo_available": runtime_health.cargo_available,
            "ffmpeg_available": runtime_health.ffmpeg_available,
            "ffprobe_available": runtime_health.ffprobe_available,
            "cmake_available": runtime_health.cmake_available,
            "magick_available": runtime_health.magick_available,
            "uv_available": runtime_health.uv_available,
            "codex_available": runtime_health.codex_available,
            "transcription_engine_ready": runtime_health.transcription_engine_ready,
            "cleanup_ready": runtime_health.cleanup_ready,
            "build_ready": runtime_health.build_ready,
            "fallback_renderer_ready": runtime_health.fallback_renderer_ready,
            "can_fix_with_brew": doctor_report.can_fix_with_brew,
            "model_ready": runtime_health.model_ready,
            "missing": runtime_health.missing,
            "brew_packages": doctor_report.brew_packages,
            "advisories": doctor_report.advisories,
            "dependencies": dependencies,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
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
    println!(
        "latest_stable_error = {}",
        release_error.as_deref().unwrap_or("none")
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
    println!("uv_available = {}", runtime_health.uv_available);
    println!("codex_available = {}", runtime_health.codex_available);
    println!(
        "transcription_engine_ready = {}",
        runtime_health.transcription_engine_ready
    );
    println!("cleanup_ready = {}", runtime_health.cleanup_ready);
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
    for status in &doctor_report.statuses {
        println!(
            "dependency.{} = {}",
            status.dependency.name,
            dependency_state(status)
        );
        println!(
            "dependency.{}.path = {}",
            status.dependency.name,
            status
                .path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "unavailable".to_string())
        );
        println!(
            "dependency.{}.version = {}",
            status.dependency.name,
            status.version.as_deref().unwrap_or("unavailable")
        );
        println!(
            "dependency.{}.error = {}",
            status.dependency.name,
            status.error.as_deref().unwrap_or("none")
        );
    }

    Ok(())
}

fn dependency_state(status: &DependencyStatus) -> &'static str {
    if status.healthy {
        "ok"
    } else if status.available {
        "unhealthy"
    } else {
        "missing"
    }
}
