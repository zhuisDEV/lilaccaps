use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct Fixture(pub PathBuf);

impl Fixture {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lilaccaps-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lilaccaps"));
        command
            .current_dir(&self.0)
            .env("LILACCAPS_HOME", self.0.join("runtime"))
            .env_remove("LILACCAPS_INSTALL_ROOT");
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
