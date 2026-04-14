use anyhow::Result;

use crate::cli::DoctorArgs;
use crate::config::load_config;
use crate::runtime::{
    collect_doctor_report, detect_runtime_health, fix_missing_dependencies_with_brew,
};

pub fn run(args: DoctorArgs) -> Result<()> {
    let loaded = load_config(args.config_path)?;
    let mut report = collect_doctor_report();
    let mut fixed_packages = Vec::new();

    if args.fix && !report.brew_packages.is_empty() {
        fixed_packages = fix_missing_dependencies_with_brew(&report)?;
        report = collect_doctor_report();
    }

    let runtime_health = detect_runtime_health(&loaded.paths, &loaded.config);

    println!("command = doctor");
    println!("fix_requested = {}", args.fix);
    println!(
        "fixed_packages = {}",
        if fixed_packages.is_empty() {
            "none".to_string()
        } else {
            fixed_packages.join(", ")
        }
    );
    println!(
        "missing_commands = {}",
        if report.missing_commands.is_empty() {
            "none".to_string()
        } else {
            report.missing_commands.join(", ")
        }
    );
    println!(
        "brew_packages = {}",
        if report.brew_packages.is_empty() {
            "none".to_string()
        } else {
            report.brew_packages.join(", ")
        }
    );
    println!("can_fix_with_brew = {}", report.can_fix_with_brew);
    println!("healthy = {}", runtime_health.healthy);
    println!("build_ready = {}", runtime_health.build_ready);
    println!(
        "fallback_renderer_ready = {}",
        runtime_health.fallback_renderer_ready
    );
    println!(
        "advisories = {}",
        if report.advisories.is_empty() {
            "none".to_string()
        } else {
            report.advisories.join(" | ")
        }
    );

    for status in report.statuses {
        println!(
            "dependency.{} = {}",
            status.dependency.name,
            if status.available { "ok" } else { "missing" }
        );
    }

    Ok(())
}
