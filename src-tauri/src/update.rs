//! GitHub Releases update check.
//!
//! Flow ships outside the App Store, so it checks GitHub Releases for a newer
//! build itself. The check is deliberately *silent and best-effort*: every
//! failure path — no network, a rate-limited API, a malformed body, an
//! unparseable tag, or simply being up to date — resolves to `None`. The user
//! is only ever notified when there is genuinely a newer release to install.
//!
//! Two pieces are pure and exhaustively unit-tested: [`is_newer`] (semver
//! comparison tolerating a leading `v`) and [`parse_release`] (extracting the
//! fields from the GitHub JSON). The network call in [`fetch_latest`] composes
//! them.

use serde::Serialize;

/// GitHub "latest release" endpoint for this repository.
const RELEASES_URL: &str = "https://api.github.com/repos/jgvilchezc/flow/releases/latest";
/// GitHub requires a User-Agent header on every API request.
const USER_AGENT: &str = "flow-updater";

/// A newer release available on GitHub. Serialized to the frontend as the
/// `flow://update-available` event payload and as the `check_for_update`
/// command result.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    /// Semver of the latest release, with any leading `v` stripped.
    pub version: String,
    /// Browser URL of the release page.
    pub url: String,
    /// Release notes (the GitHub release body), when present.
    pub notes: Option<String>,
}

/// The subset of the GitHub latest-release JSON this check cares about.
struct Release {
    tag_name: String,
    html_url: String,
    body: Option<String>,
}

/// Parses the GitHub latest-release response body.
///
/// Returns `None` on malformed JSON or a missing/blank `tag_name`; `html_url`
/// and `body` are optional and default to empty / `None`. The check is
/// best-effort, so a parse failure is a quiet `None`, never an error.
fn parse_release(body: &str) -> Option<Release> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag_name = value.get("tag_name")?.as_str()?.trim().to_string();
    if tag_name.is_empty() {
        return None;
    }
    let html_url = value
        .get("html_url")
        .and_then(|u| u.as_str())
        .unwrap_or_default()
        .to_string();
    let notes = value
        .get("body")
        .and_then(|b| b.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(Release {
        tag_name,
        html_url,
        body: notes,
    })
}

/// Compares two version strings, tolerating a leading `v`/`V` on either side.
///
/// Returns `Some(latest)` only when `latest_tag` parses to a strictly greater
/// semver than `current`. Any parse failure on either side — or an equal/older
/// latest — yields `None`.
pub fn is_newer(current: &str, latest_tag: &str) -> Option<semver::Version> {
    let strip = |s: &str| s.trim_start_matches(['v', 'V']).to_string();
    let current = semver::Version::parse(&strip(current)).ok()?;
    let latest = semver::Version::parse(&strip(latest_tag)).ok()?;
    if latest > current {
        Some(latest)
    } else {
        None
    }
}

/// Fetches the latest GitHub release and returns it only when strictly newer
/// than `current`. Every failure — network, non-success status, malformed
/// body, unparseable tag, or not newer — resolves to `None`.
pub async fn fetch_latest(current: &str) -> Option<UpdateInfo> {
    let response = crate::http::client()
        .get(RELEASES_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.text().await.ok()?;
    let release = parse_release(&body)?;
    let version = is_newer(current, &release.tag_name)?;
    Some(UpdateInfo {
        version: version.to_string(),
        url: release.html_url,
        notes: release.body,
    })
}

/// Checks GitHub for a newer release than the running build. Returns `None` on
/// any failure so the UI never shows an error for a routine, best-effort check.
#[tauri::command]
pub async fn check_for_update(app: tauri::AppHandle) -> Option<UpdateInfo> {
    let current = app.package_info().version.to_string();
    fetch_latest(&current).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_detects_a_greater_version() {
        let v = is_newer("0.1.0", "0.2.0").expect("0.2.0 is newer than 0.1.0");
        assert_eq!(v, semver::Version::parse("0.2.0").unwrap());
    }

    #[test]
    fn is_newer_equal_is_none() {
        assert!(is_newer("1.2.3", "1.2.3").is_none());
    }

    #[test]
    fn is_newer_older_is_none() {
        assert!(is_newer("2.0.0", "1.9.9").is_none());
    }

    #[test]
    fn is_newer_tolerates_v_prefix_on_both_sides() {
        let v = is_newer("v1.0.0", "v1.0.1").expect("v-prefixed newer resolves");
        assert_eq!(v, semver::Version::parse("1.0.1").unwrap());
        // Mixed prefixing still compares by value.
        assert!(is_newer("V1.0.0", "1.0.0").is_none());
    }

    #[test]
    fn is_newer_non_semver_tag_is_none() {
        assert!(is_newer("1.0.0", "latest").is_none());
        assert!(is_newer("1.0.0", "v").is_none());
    }

    #[test]
    fn is_newer_garbage_current_is_none() {
        assert!(is_newer("not-a-version", "1.0.0").is_none());
    }

    #[test]
    fn parse_release_extracts_all_fields() {
        let json = r#"{
            "tag_name": "v0.2.0",
            "html_url": "https://github.com/jgvilchezc/flow/releases/tag/v0.2.0",
            "body": "Bug fixes and speed."
        }"#;
        let release = parse_release(json).expect("valid release JSON parses");
        assert_eq!(release.tag_name, "v0.2.0");
        assert_eq!(
            release.html_url,
            "https://github.com/jgvilchezc/flow/releases/tag/v0.2.0"
        );
        assert_eq!(release.body.as_deref(), Some("Bug fixes and speed."));
    }

    #[test]
    fn parse_release_missing_tag_name_is_none() {
        let json = r#"{ "html_url": "https://example.com", "body": "notes" }"#;
        assert!(parse_release(json).is_none());
    }

    #[test]
    fn parse_release_blank_tag_name_is_none() {
        let json = r#"{ "tag_name": "   " }"#;
        assert!(parse_release(json).is_none());
    }

    #[test]
    fn parse_release_malformed_json_is_none() {
        assert!(parse_release("{ not json").is_none());
        assert!(parse_release("").is_none());
    }

    #[test]
    fn parse_release_absent_body_is_none_notes() {
        let json = r#"{ "tag_name": "v1.0.0", "html_url": "https://example.com" }"#;
        let release = parse_release(json).unwrap();
        assert_eq!(release.body, None);
    }
}
