use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::config::TranslateConfig;
use crate::runtime::{ScopedTempPath, ensure_dir, tmp_dir};

const EXECUTABLE_BUSY_RETRY_LIMIT: Duration = Duration::from_millis(250);
const EXECUTABLE_BUSY_RETRY_INTERVAL: Duration = Duration::from_millis(10);

pub fn validate_config(config: &TranslateConfig) -> Result<()> {
    validate_agent_config(
        &config.command,
        &config.model,
        &config.reasoning_effort,
        "translate",
    )?;
    validate_model(&config.review_model, "translate.review_model")?;
    validate_effort(
        &config.review_reasoning_effort,
        "translate.review_reasoning_effort",
    )
}

pub(crate) fn validate_agent_config(
    command: &str,
    model: &str,
    effort: &str,
    section: &str,
) -> Result<()> {
    let command = Path::new(command);
    if command.as_os_str().is_empty()
        || command.to_string_lossy().trim().is_empty()
        || (command.components().count() > 1 && !command.is_absolute())
    {
        bail!("{section}.command must be an executable name or absolute path");
    }
    validate_model(model, &format!("{section}.model"))?;
    validate_effort(effort, &format!("{section}.reasoning_effort"))
}

fn validate_model(model: &str, field: &str) -> Result<()> {
    let model = model_name(model);
    if model.is_empty()
        || model.starts_with("gemini-")
        || !model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        bail!("invalid Codex model in {field}");
    }
    Ok(())
}

fn validate_effort(effort: &str, field: &str) -> Result<()> {
    if !matches!(effort, "low" | "medium" | "high" | "xhigh" | "max") {
        bail!("{field} must be low, medium, high, xhigh, or max");
    }
    Ok(())
}

pub(crate) fn model_name(model: &str) -> &str {
    model
        .strip_prefix("openai/")
        .or_else(|| model.strip_prefix("codex/"))
        .unwrap_or(model)
}

/// Run one isolated structured request. Output and process diagnostics may contain
/// private captions, so neither is included in errors or printed to the terminal.
pub(crate) fn run_codex(
    runtime_home: &Path,
    command: &str,
    model: &str,
    effort: &str,
    schema: &str,
    prompt: String,
    timeout: Duration,
) -> Result<String> {
    let temp_root = tmp_dir(runtime_home);
    ensure_dir(&temp_root)?;
    let workdir = ScopedTempPath::directory(&temp_root, "caption-agent")?;
    let schema_path = workdir.path().join("schema.json");
    let output_path = workdir.path().join("output.json");
    fs::write(&schema_path, schema).context("failed to write caption review schema")?;
    // Keep the user's OAuth store, but exclude unrelated model, tool and MCP settings.
    let mut process = Command::new(command);
    process
        .args([
            "exec",
            "--ignore-user-config",
            "--sandbox",
            "read-only",
            "--ephemeral",
            "--skip-git-repo-check",
        ])
        .args([
            "--config",
            "model_provider=\"openai\"",
            "--config",
            "forced_login_method=\"chatgpt\"",
        ])
        .arg("--config")
        .arg(format!("model_reasoning_effort=\"{effort}\""))
        .arg("--output-schema")
        .arg(&schema_path)
        .arg("--output-last-message")
        .arg(&output_path)
        .arg("--model")
        .arg(model_name(model))
        .arg("-")
        .current_dir(workdir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // The npm Codex executable is a launcher for a native child. Give this request
    // its own process group so cancellation stops both and any inherited children.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    let started = Instant::now();
    let mut child = spawn_caption_agent(&mut process, started, timeout)?;
    let mut stdin = child.stdin.take().expect("piped caption agent stdin");
    let (sender, receiver) = std::sync::mpsc::channel();
    // Prompt delivery can block too; supervise it under the subprocess deadline.
    std::thread::spawn(move || {
        let result = stdin.write_all(prompt.as_bytes());
        drop(stdin);
        let _ = sender.send(result);
    });
    let status = loop {
        if let Ok(Err(_)) = receiver.try_recv() {
            terminate_request(&mut child);
            bail!("failed to send caption agent prompt");
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                terminate_request(&mut child);
                bail!("failed while waiting for caption agent");
            }
        }
        if started.elapsed() > timeout {
            terminate_request(&mut child);
            bail!(
                "Codex caption agent timed out after {} seconds",
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    if !status.success() {
        terminate_request(&mut child);
        bail!(
            "Codex caption agent failed with {status}; check `codex login status`, model access, usage limits and CLI version"
        );
    }
    fs::read_to_string(&output_path).map_err(|_| {
        terminate_request(&mut child);
        anyhow::anyhow!("Codex did not produce caption agent output")
    })
}

fn spawn_caption_agent(
    process: &mut Command,
    started: Instant,
    timeout: Duration,
) -> Result<Child> {
    loop {
        if started.elapsed() >= timeout {
            bail!("Codex caption agent timed out while waiting for its executable");
        }
        match process.spawn() {
            Ok(child) => return Ok(child),
            Err(error) => {
                let remaining = EXECUTABLE_BUSY_RETRY_LIMIT.saturating_sub(started.elapsed());
                if !executable_is_busy(&error) || remaining.is_zero() {
                    return Err(error).context("failed to start caption agent; install a current Codex CLI and sign in with ChatGPT");
                }
                // ETXTBSY means exec never started a request. Briefly tolerate a
                // concurrently finishing executable write, without retrying model,
                // authentication, missing-command or permission failures. The same
                // clock covers these waits, prompt delivery and request execution.
                let remaining = remaining.min(timeout.saturating_sub(started.elapsed()));
                std::thread::sleep(EXECUTABLE_BUSY_RETRY_INTERVAL.min(remaining));
            }
        }
    }
}

fn executable_is_busy(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(libc::ETXTBSY)
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

fn terminate_request(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(group) = i32::try_from(child.id()) {
        // SAFETY: process_group(0) creates an isolated group led by this owned
        // child. A negative PID targets that group, never the caller's group.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
    }
    // Non-Unix platforms currently cancel only the immediate executable.
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_model_and_effort() {
        let mut config = TranslateConfig::default();
        assert!(validate_config(&config).is_ok());
        config.model = "openai/gpt-5.6-luna".into();
        assert_eq!(model_name(&config.model), "gpt-5.6-luna");
        assert!(validate_config(&config).is_ok());
        config.reasoning_effort = "ultra".into();
        assert!(validate_config(&config).is_err());
        config.reasoning_effort = "medium".into();
        config.model = "gemini-3.1-flash-lite".into();
        assert!(validate_config(&config).is_err());
    }

    #[cfg(target_os = "linux")]
    fn busy_executable() -> (ScopedTempPath, std::path::PathBuf, fs::File) {
        use std::os::unix::fs::PermissionsExt;
        let runtime =
            ScopedTempPath::directory(&std::env::temp_dir(), "caption-busy-executable").unwrap();
        let command = runtime.path().join("mock-codex");
        fs::write(&command, "#!/bin/sh\noutput=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '--output-last-message' ]; then shift; output=\"$1\"; fi\n  shift\ndone\ncat > /dev/null\nprintf '%s' '{}' > \"$output\"\n").unwrap();
        fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
        let writer = fs::OpenOptions::new().write(true).open(&command).unwrap();
        // Holding a write-open descriptor produces real kernel ETXTBSY, instead
        // of mocking or relying on the filesystem race that exposed this bug.
        let error = Command::new(&command).spawn().unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::ETXTBSY));
        (runtime, command, writer)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn busy_executable_recovers_after_the_writer_releases_it() {
        let (runtime, command, writer) = busy_executable();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            drop(writer);
        });
        let result = run_codex(
            runtime.path(),
            command.to_str().unwrap(),
            "test",
            "medium",
            "{}",
            "prompt".into(),
            Duration::from_secs(2),
        );
        release.join().unwrap();
        assert_eq!(result.unwrap(), "{}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn persistently_busy_executable_has_a_short_spawn_retry_limit() {
        let (runtime, command, _writer) = busy_executable();
        let started = Instant::now();
        let error = run_codex(
            runtime.path(),
            command.to_str().unwrap(),
            "test",
            "medium",
            "{}",
            "prompt".into(),
            Duration::from_secs(3),
        )
        .unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(libc::ETXTBSY)
        );
        assert!(started.elapsed() >= EXECUTABLE_BUSY_RETRY_LIMIT);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn busy_executable_wait_uses_the_request_timeout() {
        let (runtime, command, _writer) = busy_executable();
        let started = Instant::now();
        let error = run_codex(
            runtime.path(),
            command.to_str().unwrap(),
            "test",
            "medium",
            "{}",
            "prompt".into(),
            Duration::from_millis(40),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("timed out while waiting for its executable")
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn missing_executable_and_permission_errors_do_not_qualify_for_spawn_retry() {
        assert!(executable_is_busy(&std::io::Error::from_raw_os_error(
            libc::ETXTBSY
        )));
        assert!(!executable_is_busy(&std::io::Error::from_raw_os_error(
            libc::ENOENT
        )));
        assert!(!executable_is_busy(&std::io::Error::from_raw_os_error(
            libc::EACCES
        )));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_covers_a_process_that_does_not_read_the_prompt() {
        use std::os::unix::fs::PermissionsExt;
        let runtime = ScopedTempPath::directory(&std::env::temp_dir(), "caption-timeout").unwrap();
        let command = runtime.path().join("mock-codex");
        fs::write(&command, "#!/bin/sh\nexec sleep 10\n").unwrap();
        fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        let error = run_codex(
            runtime.path(),
            command.to_str().unwrap(),
            "test",
            "medium",
            "{}",
            "a".repeat(1_000_000),
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_and_launcher_failure_stop_the_native_child_and_prevent_late_writes() {
        use std::os::unix::fs::PermissionsExt;
        for fail_launcher in [false, true] {
            let runtime =
                ScopedTempPath::directory(&std::env::temp_dir(), "caption-child-timeout").unwrap();
            let command = runtime.path().join("mock-codex");
            fs::write(&command, format!(r#"#!/usr/bin/env python3
import pathlib, subprocess, sys, time
root = pathlib.Path(__file__).parent
sys.stdin.read()
child = subprocess.Popen([sys.executable, "-c", "import pathlib,sys,time; time.sleep(1); pathlib.Path(sys.argv[1]).write_text('request still running')", str(root / "late-write")])
(root / "child.pid").write_text(str(child.pid))
if {fail_launcher}:
    sys.exit(7)
child.wait()
"#, fail_launcher = if fail_launcher { "True" } else { "False" })).unwrap();
            fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
            let error = run_codex(
                runtime.path(),
                command.to_str().unwrap(),
                "test",
                "medium",
                "{}",
                "prompt".into(),
                Duration::from_millis(300),
            )
            .unwrap_err();
            assert!(error.to_string().contains(if fail_launcher {
                "failed with"
            } else {
                "timed out"
            }));
            let child: libc::pid_t = fs::read_to_string(runtime.path().join("child.pid"))
                .unwrap()
                .parse()
                .unwrap();
            let running = child_is_running(child);
            if running {
                // Avoid leaving a request alive even when this regression fails.
                unsafe {
                    libc::kill(child, libc::SIGKILL);
                }
            }
            assert!(
                !running,
                "launcher child is still running after request cancellation"
            );
            std::thread::sleep(Duration::from_millis(1_050));
            assert!(!runtime.path().join("late-write").exists());
        }
    }

    #[cfg(unix)]
    fn child_is_running(pid: libc::pid_t) -> bool {
        for _ in 0..20 {
            // A killed grandchild may briefly await init's reaper. A zombie cannot
            // execute or write; do not mistake that state for a live request.
            #[cfg(target_os = "linux")]
            if fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|status| {
                status
                    .rsplit_once(')')
                    .is_some_and(|(_, fields)| fields.trim_start().starts_with('Z'))
            }) {
                return false;
            }
            if unsafe { libc::kill(pid, 0) } != 0 {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }
}
