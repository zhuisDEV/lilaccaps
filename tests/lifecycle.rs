mod common;

use common::Fixture;
use std::fs;
use std::process::Command;

#[test]
fn missing_explicit_uninstall_config_is_rejected_before_preview() {
    let fixture = Fixture::new();
    let result = fixture
        .command()
        .args(["uninstall", "--config-path", "missing.toml"])
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success());
    assert!(error.contains("failed to read config file missing.toml"));
    assert!(!error.contains("runtime_home ="));
}

#[cfg(unix)]
#[test]
fn new_config_ignores_the_current_checkout_remote() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let tools = fixture.0.join("tools");
    fs::create_dir(&tools).unwrap();
    let git = tools.join("git");
    fs::write(
        &git,
        "#!/bin/sh\nprintf '%s\\n' 'https://github.com/example/unrelated-project.git'\n",
    )
    .unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    // Only git is available: installation must stop after config creation, before
    // binary, model or skill writes. The old inference would persist the fake origin.
    let result = fixture
        .command()
        .env("PATH", &tools)
        .args(["install", "--config-path", "new.toml"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("prerequisite check failed"));
    let config = fs::read_to_string(fixture.0.join("new.toml")).unwrap();
    assert!(config.contains("github_repo = \"zhuisDEV/lilaccaps\""));
}

#[cfg(unix)]
#[test]
fn lifecycle_follows_the_custom_binary_through_a_launcher_symlink() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let bin = fixture.0.join("custom/bin");
    let launcher_dir = fixture.0.join("launcher/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&launcher_dir).unwrap();
    let installed = bin.join("lilaccaps");
    fs::copy(env!("CARGO_BIN_EXE_lilaccaps"), &installed).unwrap();
    let launcher = launcher_dir.join("lilaccaps");
    symlink(&installed, &launcher).unwrap();
    let config = fixture.0.join("config.toml");
    fs::write(&config, format!(
        "[runtime]\nhome = {:?}\n[agent]\nskill_path = {:?}\n[release]\n[transcribe.model]\nid = \"base\"\n",
        fixture.0.join("runtime"), fixture.0.join("SKILL.md"),
    )).unwrap();
    for override_root in [None, Some(fixture.0.join("override"))] {
        let mut command = Command::new(&launcher);
        command
            .env("CARGO_HOME", fixture.0.join("ordinary"))
            .env_remove("LILACCAPS_INSTALL_ROOT")
            .env_remove("LILACCAPS_HOME")
            .arg("uninstall")
            .arg("--config-path")
            .arg(&config);
        if let Some(root) = &override_root {
            command.env("LILACCAPS_INSTALL_ROOT", root);
        }
        let result = command.output().unwrap();
        assert!(!result.status.success());
        let expected = override_root.unwrap_or_else(|| fixture.0.join("custom"));
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(&format!(
                "binary_path = {}",
                expected.join("bin/lilaccaps").display()
            )),
            "{}",
            String::from_utf8_lossy(&result.stderr),
        );
        assert!(installed.metadata().unwrap().len() > 0);
    }
}
