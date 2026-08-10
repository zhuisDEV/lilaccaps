use std::time::Duration;

use anyhow::Result;
use semver::Version;

use crate::release::latest_release_with_timeouts;

const VERSION_CHECK_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const VERSION_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

pub fn run() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("lilaccaps {current}");

    let Ok(Some(release)) =
        latest_release_with_timeouts(None, VERSION_CHECK_CONNECT_TIMEOUT, VERSION_CHECK_TIMEOUT)
    else {
        return Ok(());
    };
    if let Some(notice) = update_notice(current, &release.version).unwrap_or(None) {
        for line in notice {
            println!("{line}");
        }
    }

    Ok(())
}

fn update_available(current: &str, latest: &str) -> Result<bool, semver::Error> {
    Ok(Version::parse(latest)? > Version::parse(current)?)
}

fn update_notice(current: &str, latest: &str) -> Result<Option<[String; 2]>, semver::Error> {
    Ok(update_available(current, latest)?.then(|| {
        [
            format!("new_version = {latest}"),
            "recommendation = run `lilaccaps update`".to_string(),
        ]
    }))
}

#[cfg(test)]
mod tests {
    use super::{update_available, update_notice};

    #[test]
    fn recommends_only_strictly_newer_semantic_versions() {
        assert!(update_available("0.1.20", "0.1.21").expect("versions should parse"));
        assert!(update_available("0.1.20", "1.0.0").expect("versions should parse"));
        assert!(!update_available("0.1.20", "0.1.20").expect("versions should parse"));
        assert!(!update_available("0.1.20", "0.1.19").expect("versions should parse"));
    }

    #[test]
    fn malformed_release_versions_do_not_compare() {
        assert!(update_available("0.1.20", "nightly").is_err());
    }

    #[test]
    fn newer_release_notice_has_the_expected_version_and_command() {
        assert_eq!(
            update_notice("0.1.20", "0.1.21").expect("versions should parse"),
            Some([
                "new_version = 0.1.21".to_string(),
                "recommendation = run `lilaccaps update`".to_string(),
            ])
        );
        assert_eq!(
            update_notice("0.1.20", "0.1.20").expect("versions should parse"),
            None
        );
    }
}
