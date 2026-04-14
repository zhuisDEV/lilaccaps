use std::process::Command;

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    prerelease: bool,
    draft: bool,
}

pub fn latest_release(repo: Option<&str>) -> Result<Option<ReleaseInfo>> {
    let Some(repo) = repo else {
        return Ok(None);
    };

    let normalized = normalize_github_repo(repo)?;
    let url = format!("https://api.github.com/repos/{normalized}/releases/latest");
    let client = Client::builder()
        .user_agent(format!("lilaccaps/{}", env!("CARGO_PKG_VERSION")))
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
    if input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("git@")
        || input.starts_with("ssh://")
    {
        return normalize_github_remote_url(input);
    }

    let trimmed = input.trim().trim_end_matches(".git");
    if trimmed.split('/').count() != 2 {
        bail!("invalid GitHub repo identifier: {input}");
    }

    Ok(trimmed.to_string())
}

fn normalize_github_remote_url(input: &str) -> Result<String> {
    let trimmed = input.trim().trim_end_matches(".git");

    if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        return Ok(rest.to_string());
    }

    if let Some(rest) = trimmed.strip_prefix("https://github.com/") {
        return Ok(rest.to_string());
    }

    if let Some(rest) = trimmed.strip_prefix("http://github.com/") {
        return Ok(rest.to_string());
    }

    if let Some(rest) = trimmed.strip_prefix("ssh://git@github.com/") {
        return Ok(rest.to_string());
    }

    bail!("unsupported GitHub remote URL: {input}")
}

#[cfg(test)]
mod tests {
    use super::normalize_github_repo;

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
}
