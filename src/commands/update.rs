use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::cli::UpdateArgs;
use crate::config::load_or_init_config;
use crate::release::{ReleaseInfo, default_github_repo, latest_release, normalize_github_repo};
use crate::runtime::{
    CARGO_DEPENDENCY, CMAKE_DEPENDENCY, DependencyUpdateReport, cargo_install_root,
    ensure_dependency, install_binary_path, update_dependencies_with_brew,
};

pub fn run(args: UpdateArgs) -> Result<()> {
    ensure_dependency(CARGO_DEPENDENCY)?;
    let config_path = args.config_path.clone();
    let loaded = load_or_init_config(config_path.clone())?;
    let repo = loaded
        .config
        .release
        .github_repo
        .clone()
        .unwrap_or_else(default_github_repo);
    let normalized_repo = normalize_github_repo(&repo)?;
    let release = latest_release(Some(&normalized_repo))?
        .ok_or_else(|| anyhow::anyhow!("no stable release found for {normalized_repo}"))?;

    let dependency_update = if args.skip_dependencies {
        DependencyUpdateReport {
            updated_packages: Vec::new(),
            skipped_reason: Some("dependency updates were skipped by request".to_string()),
        }
    } else {
        update_dependencies_with_brew()?
    };
    ensure_dependency(CMAKE_DEPENDENCY)?;

    let installed_binary = install_and_refresh(&normalized_repo, &release, config_path.as_deref())?;

    println!("updated = true");
    println!("repo = {}", normalized_repo);
    println!("version = {}", release.version);
    println!("tag = {}", release.tag_name);
    println!("binary_path = {}", installed_binary.display());
    println!(
        "dependency_update = {}",
        if dependency_update.skipped_reason.is_some() {
            "skipped"
        } else {
            "completed"
        }
    );
    println!(
        "dependency_packages = {}",
        if dependency_update.updated_packages.is_empty() {
            "none".to_string()
        } else {
            dependency_update.updated_packages.join(", ")
        }
    );
    println!(
        "dependency_update_reason = {}",
        dependency_update
            .skipped_reason
            .as_deref()
            .unwrap_or("none")
    );

    Ok(())
}

fn install_and_refresh(
    normalized_repo: &str,
    release: &ReleaseInfo,
    config_path: Option<&Path>,
) -> Result<PathBuf> {
    // Cargo replaces this running executable. On Linux, current_exe then points
    // at a deleted inode and cannot be canonicalised, so resolve both paths now.
    let install_root = cargo_install_root()?;
    let installed_binary = install_binary_path()?;
    let status = Command::new("cargo")
        .arg("install")
        .arg("--root")
        .arg(&install_root)
        .arg("--git")
        .arg(format!("https://github.com/{normalized_repo}.git"))
        .arg("--tag")
        .arg(&release.tag_name)
        .arg("--locked")
        .arg("--force")
        .arg("lilaccaps")
        .status()
        .with_context(|| "failed to start cargo install for lilaccaps update")?;

    if !status.success() {
        bail!("cargo install failed while updating lilaccaps");
    }

    let mut refresh = Command::new(&installed_binary);
    refresh.arg("install");
    if let Some(config_path) = config_path {
        refresh.arg("--config-path").arg(config_path);
    }
    let refresh_status = refresh.status().with_context(|| {
        format!(
            "failed to start the updated lilaccaps binary at {}",
            installed_binary.display()
        )
    })?;
    if !refresh_status.success() {
        bail!("updated lilaccaps installed but post-update setup validation failed");
    }

    Ok(installed_binary)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    use crate::runtime::ScopedTempPath;

    #[test]
    fn running_updater_refreshes_its_replacement_with_custom_config() {
        if let Some(root) = std::env::var_os("LILACCAPS_TEST_RUNNING_UPDATE") {
            let root = PathBuf::from(root);
            let config = root.join("settings/custom captions.toml");
            let installed = install_and_refresh(
                "example/lilaccaps",
                &ReleaseInfo {
                    tag_name: "v9.9.9".into(),
                    version: "9.9.9".into(),
                },
                Some(&config),
            )
            .expect("refresh must use the path resolved before replacing the running executable");
            assert_eq!(installed, root.join("custom cargo/bin/lilaccaps"));
            // This is the actual Linux failure condition; the test must not pass
            // just because a different, non-running file happened to be replaced.
            assert!(std::env::current_exe().unwrap().canonicalize().is_err());
            assert!(
                install_binary_path().is_err(),
                "a late path lookup must reproduce the original Linux failure"
            );
            return;
        }

        let fixture =
            ScopedTempPath::directory(&std::env::temp_dir(), "lilaccaps-running-update").unwrap();
        let root = fixture.path();
        let install_root = root.join("custom cargo");
        let bin = install_root.join("bin");
        let tools = root.join("tools");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&tools).unwrap();
        fs::create_dir_all(root.join("settings")).unwrap();
        let installed = bin.join("lilaccaps");
        fs::copy(std::env::current_exe().unwrap(), &installed).unwrap();
        let original_inode = fs::metadata(&installed).unwrap().ino();
        let config = root.join("settings/custom captions.toml");
        fs::write(&config, "custom configuration must remain unchanged\n").unwrap();
        let cargo = tools.join("cargo");
        fs::write(
            &cargo,
            r#"#!/usr/bin/python3
import json, os, pathlib, sys
root = pathlib.Path(os.environ["LILACCAPS_TEST_RUNNING_UPDATE"])
args = sys.argv[1:]
assert args[0] == "install"
install_root = pathlib.Path(args[args.index("--root") + 1])
assert install_root == root / "custom cargo"
(root / "cargo-args.json").write_text(json.dumps(args))
replacement = install_root / "bin/lilaccaps.new"
replacement.write_text('#!/bin/sh\nprintf \'%s\\n\' "$0" "$@" > "$LILACCAPS_TEST_REFRESH_LOG"\n')
replacement.chmod(0o755)
os.replace(replacement, install_root / "bin/lilaccaps")
"#,
        )
        .unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths([tools, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
            .unwrap();
        let result = Command::new(&installed)
            .args(["--exact", "commands::update::tests::running_updater_refreshes_its_replacement_with_custom_config", "--nocapture"])
            .current_dir(root)
            .env("PATH", path)
            .env("CARGO_HOME", root.join("unrelated cargo home"))
            .env_remove("LILACCAPS_INSTALL_ROOT")
            .env("LILACCAPS_TEST_RUNNING_UPDATE", root)
            .env("LILACCAPS_TEST_REFRESH_LOG", root.join("refresh-args"))
            .output().unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert_ne!(fs::metadata(&installed).unwrap().ino(), original_inode);
        let expected = format!(
            "{}\ninstall\n--config-path\n{}\n",
            installed.display(),
            config.display()
        );
        assert_eq!(
            fs::read_to_string(root.join("refresh-args")).unwrap(),
            expected
        );
        assert_eq!(
            fs::read_to_string(&config).unwrap(),
            "custom configuration must remain unchanged\n"
        );
        let args: Vec<String> =
            serde_json::from_str(&fs::read_to_string(root.join("cargo-args.json")).unwrap())
                .unwrap();
        assert_eq!(
            args[args.iter().position(|arg| arg == "--root").unwrap() + 1],
            install_root.to_string_lossy()
        );
        assert_eq!(
            args[args.iter().position(|arg| arg == "--tag").unwrap() + 1],
            "v9.9.9"
        );
        assert!(!root.join("unrelated cargo home").exists());
    }
}
