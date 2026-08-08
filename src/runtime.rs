use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::config::{Config, ConfigPaths};

#[derive(Debug, Clone, Copy)]
pub struct CommandDependency {
    pub name: &'static str,
    pub purpose: &'static str,
    pub install_hint: &'static str,
    pub brew_package: Option<&'static str>,
    pub version_args: &'static [&'static str],
}

pub const CARGO_DEPENDENCY: CommandDependency = CommandDependency {
    name: "cargo",
    purpose: "build and update the lilaccaps binary from source",
    install_hint: "Install the Rust toolchain from https://rustup.rs",
    brew_package: None,
    version_args: &["--version"],
};

pub const FFMPEG_DEPENDENCY: CommandDependency = CommandDependency {
    name: "ffmpeg",
    purpose: "extract audio and render video output",
    install_hint: "On macOS with Homebrew: brew install ffmpeg-full",
    brew_package: Some("ffmpeg-full"),
    version_args: &["-version"],
};

pub const FFPROBE_DEPENDENCY: CommandDependency = CommandDependency {
    name: "ffprobe",
    purpose: "inspect media streams and dimensions",
    install_hint: "On macOS with Homebrew: brew install ffmpeg-full",
    brew_package: Some("ffmpeg-full"),
    version_args: &["-version"],
};

pub const CMAKE_DEPENDENCY: CommandDependency = CommandDependency {
    name: "cmake",
    purpose: "build whisper-rs and whisper.cpp during cargo install/update",
    install_hint: "On macOS with Homebrew: brew install cmake",
    brew_package: Some("cmake"),
    version_args: &["--version"],
};

pub const MAGICK_DEPENDENCY: CommandDependency = CommandDependency {
    name: "magick",
    purpose: "render fallback subtitle overlays, text watermarks, and converted image watermarks",
    install_hint: "On macOS with Homebrew: brew install imagemagick",
    brew_package: Some("imagemagick"),
    version_args: &["--version"],
};

const BREW_DEPENDENCY: CommandDependency = CommandDependency {
    name: "brew",
    purpose: "install missing lilaccaps prerequisites automatically",
    install_hint: "Install Homebrew from https://brew.sh and rerun lilaccaps doctor --fix",
    brew_package: None,
    version_args: &["--version"],
};

const MANAGED_BREW_PACKAGES: [&str; 3] = ["ffmpeg-full", "cmake", "imagemagick"];
const RUNTIME_MARKER_FILE: &str = ".lilaccaps-runtime";
static TEMP_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

const ALL_DEPENDENCIES: [CommandDependency; 5] = [
    CARGO_DEPENDENCY,
    FFMPEG_DEPENDENCY,
    FFPROBE_DEPENDENCY,
    CMAKE_DEPENDENCY,
    MAGICK_DEPENDENCY,
];

#[derive(Debug, Clone)]
pub struct RuntimeHealth {
    pub installed: bool,
    pub config_valid: bool,
    pub healthy: bool,
    pub cargo_available: bool,
    pub ffmpeg_available: bool,
    pub ffprobe_available: bool,
    pub cmake_available: bool,
    pub magick_available: bool,
    pub build_ready: bool,
    pub fallback_renderer_ready: bool,
    pub model_ready: bool,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DependencyStatus {
    pub dependency: CommandDependency,
    pub available: bool,
    pub healthy: bool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub statuses: Vec<DependencyStatus>,
    pub missing_commands: Vec<String>,
    pub advisories: Vec<String>,
    pub brew_packages: Vec<String>,
    pub can_fix_with_brew: bool,
}

#[derive(Debug, Clone)]
pub struct DependencyUpdateReport {
    pub updated_packages: Vec<String>,
    pub skipped_reason: Option<String>,
}

#[derive(Debug)]
pub struct ScopedTempPath {
    path: PathBuf,
    is_dir: bool,
    active: bool,
}

impl ScopedTempPath {
    pub fn file(parent: &Path, prefix: &str, extension: Option<&str>) -> Self {
        Self {
            path: unique_temp_path(parent, prefix, extension),
            is_dir: false,
            active: true,
        }
    }

    pub fn directory(parent: &Path, prefix: &str) -> Result<Self> {
        ensure_dir(parent)?;
        let path = unique_temp_path(parent, prefix, None);
        fs::create_dir(&path)
            .with_context(|| format!("failed to create temporary directory {}", path.display()))?;
        Ok(Self {
            path,
            is_dir: true,
            active: true,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn persist(mut self, destination: &Path) -> Result<()> {
        fs::rename(&self.path, destination).with_context(|| {
            format!(
                "failed to move temporary output {} to {}",
                self.path.display(),
                destination.display()
            )
        })?;
        self.active = false;
        Ok(())
    }
}

impl Drop for ScopedTempPath {
    fn drop(&mut self) {
        if !self.active {
            return;
        }

        if self.is_dir {
            let _ = fs::remove_dir_all(&self.path);
        } else {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory {}", path.display()))
}

pub fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    ensure_dir(parent_dir(path))
}

pub fn atomic_write(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    ensure_parent_dir(path)?;
    let prefix = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("lilaccaps-output");
    let temporary = ScopedTempPath::file(parent_dir(path), prefix, Some("tmp"));
    fs::write(temporary.path(), contents).with_context(|| {
        format!(
            "failed to write temporary file {}",
            temporary.path().display()
        )
    })?;
    temporary.persist(path)
}

pub fn cargo_bin_dir() -> Result<PathBuf> {
    if let Ok(cargo_home) = env::var("CARGO_HOME") {
        return Ok(PathBuf::from(cargo_home).join("bin"));
    }

    let home = dirs::home_dir().context("failed to detect home directory")?;
    Ok(home.join(".cargo").join("bin"))
}

pub fn cargo_install_root() -> Result<PathBuf> {
    cargo_bin_dir()?
        .parent()
        .map(Path::to_path_buf)
        .context("failed to resolve Cargo install root")
}

pub fn install_binary_path() -> Result<PathBuf> {
    Ok(cargo_bin_dir()?.join("lilaccaps"))
}

pub fn current_executable() -> Result<PathBuf> {
    env::current_exe().context("failed to locate current lilaccaps executable")
}

pub fn models_dir(runtime_home: &Path) -> PathBuf {
    runtime_home.join("models")
}

pub fn tmp_dir(runtime_home: &Path) -> PathBuf {
    runtime_home.join("tmp")
}

pub fn ensure_runtime_marker(runtime_home: &Path) -> Result<PathBuf> {
    let marker = runtime_home.join(RUNTIME_MARKER_FILE);
    atomic_write(&marker, "managed by lilaccaps\n")
        .with_context(|| format!("failed to write runtime marker {}", marker.display()))?;
    Ok(marker)
}

pub fn runtime_marker_path(runtime_home: &Path) -> PathBuf {
    runtime_home.join(RUNTIME_MARKER_FILE)
}

pub fn unique_temp_path(parent: &Path, prefix: &str, extension: Option<&str>) -> PathBuf {
    let sequence = TEMP_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut name = format!(".{prefix}-{}-{timestamp}-{sequence}", std::process::id());
    if let Some(extension) = extension.filter(|value| !value.is_empty()) {
        name.push('.');
        name.push_str(extension.trim_start_matches('.'));
    }
    parent.join(name)
}

pub fn paths_refer_to_same_file(left: &Path, right: &Path) -> Result<bool> {
    if left == right {
        return Ok(true);
    }
    if !left.exists() || !right.exists() {
        return Ok(false);
    }

    let left = fs::canonicalize(left)
        .with_context(|| format!("failed to canonicalize {}", left.display()))?;
    let right = fs::canonicalize(right)
        .with_context(|| format!("failed to canonicalize {}", right.display()))?;
    if left == right {
        return Ok(true);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let left_metadata =
            fs::metadata(&left).with_context(|| format!("failed to inspect {}", left.display()))?;
        let right_metadata = fs::metadata(&right)
            .with_context(|| format!("failed to inspect {}", right.display()))?;
        Ok(left_metadata.dev() == right_metadata.dev()
            && left_metadata.ino() == right_metadata.ino())
    }

    #[cfg(not(unix))]
    {
        Ok(false)
    }
}

pub fn validate_runtime_home_for_removal(runtime_home: &Path) -> Result<PathBuf> {
    if !runtime_home.is_absolute() {
        bail!(
            "refusing to remove non-absolute runtime home: {}",
            runtime_home.display()
        );
    }

    let metadata = fs::symlink_metadata(runtime_home)
        .with_context(|| format!("failed to inspect runtime home {}", runtime_home.display()))?;
    if metadata.file_type().is_symlink() {
        bail!(
            "refusing to recursively remove symlinked runtime home: {}",
            runtime_home.display()
        );
    }
    if !metadata.is_dir() {
        bail!(
            "runtime home is not a directory: {}",
            runtime_home.display()
        );
    }

    let canonical = fs::canonicalize(runtime_home).with_context(|| {
        format!(
            "failed to canonicalize runtime home {}",
            runtime_home.display()
        )
    })?;
    let home = dirs::home_dir()
        .context("failed to detect home directory")?
        .canonicalize()
        .context("failed to canonicalize home directory")?;
    let current = env::current_dir()
        .context("failed to detect current directory")?
        .canonicalize()
        .context("failed to canonicalize current directory")?;

    if canonical.parent().is_none()
        || home == canonical
        || home.starts_with(&canonical)
        || current == canonical
        || current.starts_with(&canonical)
    {
        bail!(
            "refusing to remove protected runtime home: {}",
            canonical.display()
        );
    }
    let normal_component_count = canonical
        .components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count();
    let has_conventional_name = canonical
        .file_name()
        .is_some_and(|name| name == "lilaccaps");
    if normal_component_count < 2 || (normal_component_count < 4 && !has_conventional_name) {
        bail!(
            "refusing to remove shallow runtime home: {}",
            canonical.display()
        );
    }

    let default_runtime = home.join(".lilac").join("lilaccaps");
    if !runtime_marker_path(&canonical).is_file() && canonical != default_runtime {
        bail!(
            "refusing to remove unowned runtime home without {}: {}",
            RUNTIME_MARKER_FILE,
            canonical.display()
        );
    }

    Ok(canonical)
}

pub fn command_exists(name: &str) -> bool {
    command_path(name).is_some()
}

pub fn command_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

pub fn ensure_dependency(dep: CommandDependency) -> Result<()> {
    let status = probe_dependency(dep);
    if status.healthy {
        return Ok(());
    }

    if let Some(error) = status.error.as_deref() {
        let path = status
            .path
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| dep.name.to_string());
        bail!(
            "{} at {} is required to {} but failed its health check: {}. {}",
            dep.name,
            path,
            dep.purpose,
            error,
            dep.install_hint
        );
    }

    bail!(
        "{} is required to {} but was not found on PATH. {}",
        dep.name,
        dep.purpose,
        dep.install_hint
    );
}

pub fn collect_dependency_statuses() -> Vec<DependencyStatus> {
    ALL_DEPENDENCIES
        .iter()
        .copied()
        .map(probe_dependency)
        .collect()
}

pub fn probe_dependency(dependency: CommandDependency) -> DependencyStatus {
    let Some(path) = command_path(dependency.name) else {
        return DependencyStatus {
            dependency,
            available: false,
            healthy: false,
            path: None,
            version: None,
            error: None,
        };
    };

    match Command::new(&path).args(dependency.version_args).output() {
        Ok(output) => {
            let version =
                first_output_line(&output.stdout).or_else(|| first_output_line(&output.stderr));
            if output.status.success() {
                DependencyStatus {
                    dependency,
                    available: true,
                    healthy: true,
                    path: Some(path),
                    version,
                    error: None,
                }
            } else {
                DependencyStatus {
                    dependency,
                    available: true,
                    healthy: false,
                    path: Some(path),
                    version: None,
                    error: Some(
                        version.unwrap_or_else(|| format!("exited with status {}", output.status)),
                    ),
                }
            }
        }
        Err(error) => DependencyStatus {
            dependency,
            available: true,
            healthy: false,
            path: Some(path),
            version: None,
            error: Some(error.to_string()),
        },
    }
}

fn first_output_line(output: &[u8]) -> Option<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

pub fn collect_doctor_report() -> DoctorReport {
    let statuses = collect_dependency_statuses();
    let mut missing_commands = Vec::new();
    let mut advisories = Vec::new();
    let mut brew_packages = Vec::new();

    for status in &statuses {
        if status.healthy {
            continue;
        }

        missing_commands.push(status.dependency.name.to_string());
        advisories.push(if let Some(error) = &status.error {
            let path = status
                .path
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| status.dependency.name.to_string());
            format!(
                "{} at {} was found but failed its health check: {}. {}",
                status.dependency.name, path, error, status.dependency.install_hint
            )
        } else {
            format!(
                "{} is required to {} but was not found on PATH. {}",
                status.dependency.name, status.dependency.purpose, status.dependency.install_hint
            )
        });

        if let Some(package) = status.dependency.brew_package
            && !brew_packages.iter().any(|item| item == package)
        {
            brew_packages.push(package.to_string());
        }
    }

    let can_fix_with_brew = cfg!(target_os = "macos")
        && command_exists(BREW_DEPENDENCY.name)
        && !brew_packages.is_empty();

    DoctorReport {
        statuses,
        missing_commands,
        advisories,
        brew_packages,
        can_fix_with_brew,
    }
}

pub fn fix_dependencies_with_brew(report: &DoctorReport) -> Result<Vec<String>> {
    if report.brew_packages.is_empty() {
        return Ok(Vec::new());
    }

    if !cfg!(target_os = "macos") {
        bail!("automatic prerequisite installation is only supported on macOS with Homebrew");
    }

    ensure_dependency(BREW_DEPENDENCY)?;

    for package in &report.brew_packages {
        let action = if brew_package_installed(package)? {
            "reinstall"
        } else {
            "install"
        };
        run_brew(&[action, package], "repairing lilaccaps prerequisites")?;
    }

    relink_ffmpeg_full_if_installed()?;

    Ok(report.brew_packages.clone())
}

pub fn update_dependencies_with_brew() -> Result<DependencyUpdateReport> {
    if !cfg!(target_os = "macos") {
        return Ok(DependencyUpdateReport {
            updated_packages: Vec::new(),
            skipped_reason: Some(
                "automatic dependency updates are only supported on macOS with Homebrew"
                    .to_string(),
            ),
        });
    }

    if !command_exists(BREW_DEPENDENCY.name) {
        return Ok(DependencyUpdateReport {
            updated_packages: Vec::new(),
            skipped_reason: Some("Homebrew is not available".to_string()),
        });
    }

    run_brew(&["update"], "refreshing Homebrew package metadata")?;

    let mut updated_packages = Vec::new();
    for package in MANAGED_BREW_PACKAGES {
        let action = if brew_package_installed(package)? {
            "upgrade"
        } else {
            "install"
        };
        run_brew(&[action, package], "updating lilaccaps dependencies")?;
        updated_packages.push(package.to_string());
    }

    relink_ffmpeg_full_if_installed()?;

    let unhealthy = collect_dependency_statuses()
        .into_iter()
        .filter(|status| !status.healthy)
        .map(|status| status.dependency.name)
        .collect::<Vec<_>>();
    if !unhealthy.is_empty() {
        bail!(
            "dependency update completed but these commands are still unhealthy: {}",
            unhealthy.join(", ")
        );
    }

    Ok(DependencyUpdateReport {
        updated_packages,
        skipped_reason: None,
    })
}

fn brew_package_installed(package: &str) -> Result<bool> {
    let output = Command::new("brew")
        .args(["list", "--versions", package])
        .output()
        .with_context(|| format!("failed to inspect Homebrew package {package}"))?;
    Ok(output.status.success())
}

fn run_brew(args: &[&str], context: &str) -> Result<()> {
    let status = Command::new("brew")
        .args(args)
        .status()
        .with_context(|| format!("failed to start Homebrew while {context}"))?;
    if !status.success() {
        bail!("Homebrew failed while {context}: brew {}", args.join(" "));
    }
    Ok(())
}

fn relink_ffmpeg_full_if_installed() -> Result<()> {
    if !brew_package_installed("ffmpeg-full")? {
        return Ok(());
    }

    if brew_package_installed("ffmpeg")? {
        run_brew(
            &["unlink", "ffmpeg"],
            "unlinking the fallback ffmpeg package",
        )?;
    }
    run_brew(
        &["link", "--overwrite", "--force", "ffmpeg-full"],
        "linking ffmpeg-full",
    )
}

pub fn detect_runtime_health(paths: &ConfigPaths, config: &Config) -> RuntimeHealth {
    let install_path = install_binary_path().ok();
    let installed = install_path.as_ref().is_some_and(|path| path.exists());
    let cargo_available = probe_dependency(CARGO_DEPENDENCY).healthy;
    let ffmpeg_available = probe_dependency(FFMPEG_DEPENDENCY).healthy;
    let ffprobe_available = probe_dependency(FFPROBE_DEPENDENCY).healthy;
    let cmake_available = probe_dependency(CMAKE_DEPENDENCY).healthy;
    let magick_available = probe_dependency(MAGICK_DEPENDENCY).healthy;
    let model_path = crate::model::resolved_model_path(paths, config).ok();
    let model_ready = model_path.as_ref().is_some_and(|path| {
        fs::metadata(path)
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false)
    });

    let mut missing = Vec::new();
    if !paths.config_path.exists() {
        missing.push("config".to_string());
    }
    if !paths.runtime_home.exists() {
        missing.push("runtime_home".to_string());
    }
    if !config.agent.skill_path.exists() {
        missing.push("skill_path".to_string());
    }
    if !ffmpeg_available {
        missing.push("ffmpeg".to_string());
    }
    if !ffprobe_available {
        missing.push("ffprobe".to_string());
    }
    if !model_ready {
        missing.push("model".to_string());
    }

    RuntimeHealth {
        installed,
        config_valid: missing.iter().all(|item| item != "config"),
        healthy: installed && missing.is_empty(),
        cargo_available,
        ffmpeg_available,
        ffprobe_available,
        cmake_available,
        magick_available,
        build_ready: cargo_available && cmake_available,
        fallback_renderer_ready: magick_available,
        model_ready,
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandDependency, DoctorReport, ScopedTempPath, ensure_dependency, ensure_runtime_marker,
        first_output_line, fix_dependencies_with_brew, paths_refer_to_same_file, unique_temp_path,
        validate_runtime_home_for_removal,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "lilaccaps-runtime-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn missing_dependency_error_includes_install_hint() {
        let missing = CommandDependency {
            name: "definitely-not-a-real-command-for-lilaccaps-tests",
            purpose: "exercise dependency error messaging",
            install_hint: "Install it with the package manager used for this environment",
            brew_package: None,
            version_args: &["--version"],
        };
        let err = ensure_dependency(missing).unwrap_err();
        let message = err.to_string();
        assert!(message.contains(missing.name));
        assert!(message.contains(missing.install_hint));
    }

    #[test]
    fn unhealthy_dependency_error_includes_probe_failure() {
        let unhealthy = CommandDependency {
            name: "sh",
            purpose: "exercise dependency health checks",
            install_hint: "Repair the command",
            brew_package: None,
            version_args: &["-c", "printf 'synthetic probe failure\\n' >&2; exit 7"],
        };
        let err = ensure_dependency(unhealthy).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("failed its health check"));
        assert!(message.contains("synthetic probe failure"));
    }

    #[test]
    fn brew_fix_rejects_non_macos_without_running_brew() {
        if cfg!(target_os = "macos") {
            return;
        }

        let report = DoctorReport {
            statuses: Vec::new(),
            missing_commands: vec!["ffmpeg".to_string()],
            advisories: vec!["ffmpeg missing".to_string()],
            brew_packages: vec!["ffmpeg".to_string()],
            can_fix_with_brew: false,
        };

        let err = fix_dependencies_with_brew(&report).unwrap_err();
        assert!(
            err.to_string()
                .contains("only supported on macOS with Homebrew")
        );
    }

    #[test]
    fn extracts_first_non_empty_version_line() {
        let line = first_output_line(b"\nffmpeg version 8.1.2\nconfiguration: test\n");
        assert_eq!(line.as_deref(), Some("ffmpeg version 8.1.2"));
    }

    #[test]
    fn unique_temp_paths_do_not_collide() {
        let first = unique_temp_path(Path::new("/tmp"), "audio", Some("wav"));
        let second = unique_temp_path(Path::new("/tmp"), "audio", Some("wav"));
        assert_ne!(first, second);
        assert_eq!(
            first.extension().and_then(|value| value.to_str()),
            Some("wav")
        );
    }

    #[test]
    fn same_path_is_detected_without_touching_the_file_system() {
        assert!(
            paths_refer_to_same_file(Path::new("input.mp4"), Path::new("input.mp4"))
                .expect("matching paths should compare")
        );
    }

    #[test]
    fn hard_link_is_detected_as_same_file() {
        let dir = test_dir("hard-link");
        fs::create_dir_all(&dir).expect("test directory should be created");
        let input = dir.join("input.mp4");
        let output = dir.join("output.mp4");
        fs::write(&input, b"video").expect("test input should be written");
        fs::hard_link(&input, &output).expect("hard link should be created");

        assert!(paths_refer_to_same_file(&input, &output).expect("paths should compare"));
        fs::remove_dir_all(dir).expect("test directory should be removed");
    }

    #[test]
    fn scoped_temp_path_cleans_file_and_directory() {
        let dir = test_dir("scoped-temp");
        fs::create_dir_all(&dir).expect("test directory should be created");

        let temp_file_path = {
            let temp = ScopedTempPath::file(&dir, "output", Some("srt"));
            fs::write(temp.path(), b"temporary").expect("temporary file should be written");
            temp.path().to_path_buf()
        };
        assert!(!temp_file_path.exists());

        let temp_dir_path = {
            let temp = ScopedTempPath::directory(&dir, "overlays")
                .expect("temporary directory should be created");
            fs::write(temp.path().join("cue.png"), b"temporary")
                .expect("temporary directory content should be written");
            temp.path().to_path_buf()
        };
        assert!(!temp_dir_path.exists());

        fs::remove_dir_all(dir).expect("test directory should be removed");
    }

    #[test]
    fn runtime_removal_requires_owned_safe_directory() {
        let root = test_dir("remove");
        let runtime_home = root.join("nested").join("runtime");
        fs::create_dir_all(&runtime_home).expect("runtime home should be created");
        let error = validate_runtime_home_for_removal(&runtime_home)
            .expect_err("unmarked custom runtime should be rejected");
        assert!(error.to_string().contains("unowned runtime home"));

        ensure_runtime_marker(&runtime_home).expect("runtime marker should be written");
        let validated = validate_runtime_home_for_removal(&runtime_home)
            .expect("marked custom runtime should be accepted");
        assert_eq!(
            validated,
            runtime_home.canonicalize().expect("path should resolve")
        );

        fs::remove_dir_all(root).expect("test directory should be removed");
    }

    #[test]
    fn runtime_removal_rejects_shallow_directory_even_with_marker() {
        if !cfg!(unix) {
            return;
        }
        let unique = test_dir("shallow-remove");
        let runtime_home = Path::new("/tmp").join(
            unique
                .file_name()
                .expect("temporary test path should have a name"),
        );
        fs::create_dir_all(&runtime_home).expect("runtime home should be created");
        ensure_runtime_marker(&runtime_home).expect("runtime marker should be written");

        let error = validate_runtime_home_for_removal(&runtime_home)
            .expect_err("shallow runtime should be rejected");
        assert!(error.to_string().contains("shallow runtime home"));

        fs::remove_dir_all(runtime_home).expect("test directory should be removed");
    }
}
