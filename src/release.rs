use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
}

pub fn default_github_repo() -> String {
    normalize_github_repo(env!("CARGO_PKG_REPOSITORY"))
        .unwrap_or_else(|_| "zhuisDEV/lilaccaps".to_string())
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    prerelease: bool,
    draft: bool,
}

pub fn latest_release(repo: Option<&str>) -> Result<Option<ReleaseInfo>> {
    latest_release_with_timeouts(repo, Duration::from_secs(15), Duration::from_secs(60))
}

pub fn latest_release_with_timeouts(
    repo: Option<&str>,
    connect_timeout: Duration,
    timeout: Duration,
) -> Result<Option<ReleaseInfo>> {
    let fallback;
    let repo = match repo {
        Some(repo) => repo,
        None => {
            fallback = default_github_repo();
            &fallback
        }
    };
    let normalized = normalize_github_repo(repo)?;
    let url = format!("https://api.github.com/repos/{normalized}/releases/latest");
    let client = Client::builder()
        .user_agent(format!("lilaccaps/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(connect_timeout)
        .timeout(timeout)
        .build()
        .context("failed to construct GitHub release client")?;

    let response = client
        .get(url)
        .send()
        .context("failed to query GitHub release API")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let response = response.error_for_status()?;
    let release: GitHubRelease = response
        .json()
        .context("failed to decode GitHub release payload")?;

    if release.prerelease || release.draft {
        return Ok(None);
    }

    let version = release.tag_name.trim_start_matches('v').to_string();
    Ok(Some(ReleaseInfo {
        tag_name: release.tag_name,
        version,
    }))
}

pub fn infer_github_repo() -> Option<String> {
    let output = Command::new("git")
        .arg("remote")
        .arg("get-url")
        .arg("origin")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    normalize_github_remote_url(&raw).ok()
}

pub fn normalize_github_repo(input: &str) -> Result<String> {
    let input = input.trim();
    if input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("git@")
        || input.starts_with("ssh://")
    {
        return normalize_github_remote_url(input);
    }

    normalize_github_slug(input)
}

fn normalize_github_remote_url(input: &str) -> Result<String> {
    let trimmed = input.trim().trim_end_matches(".git");

    if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        return normalize_github_slug(rest);
    }

    if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        return normalize_github_slug(rest);
    }

    if let Some(rest) = trimmed.strip_prefix("http://github.com/") {
        return normalize_github_slug(rest);
    }

    if let Some(rest) = trimmed.strip_prefix("ssh://git@github.com/") {
        return normalize_github_slug(rest);
    }

    bail!("unsupported GitHub remote URL: {input}")
}

fn normalize_github_slug(input: &str) -> Result<String> {
    let trimmed = input.trim().trim_end_matches(".git");
    let parts = trimmed.split('/').collect::<Vec<_>>();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || matches!(*part, "." | "..")
                || !part.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
        })
    {
        bail!("invalid GitHub repo identifier: {input}");
    }

    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{default_github_repo, normalize_github_repo};

    #[test]
    fn normalizes_short_repo() {
        let repo = normalize_github_repo("owner/name").expect("short repo should normalize");
        assert_eq!(repo, "owner/name");
    }

    #[test]
    fn normalizes_https_remote() {
        let repo = normalize_github_repo("https://github.com/owner/name.git")
            .expect("https remote should normalize");
        assert_eq!(repo, "owner/name");
    }

    #[test]
    fn normalizes_ssh_remote() {
        let repo = normalize_github_repo("git@github.com:owner/name.git")
            .expect("ssh remote should normalize");
        assert_eq!(repo, "owner/name");
    }

    #[test]
    fn package_repository_has_stable_update_fallback() {
        assert_eq!(default_github_repo(), "zhuisDEV/lilaccaps");
    }

    #[test]
    fn rejects_malformed_repo_identifiers() {
        assert!(normalize_github_repo("owner/").is_err());
        assert!(normalize_github_repo("owner/name/extra").is_err());
        assert!(normalize_github_repo("owner/name?token=secret").is_err());
        assert!(normalize_github_repo("owner/..").is_err());
    }
}
