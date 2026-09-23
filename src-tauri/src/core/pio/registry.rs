//! The PlatformIO registry HTTP search API (`FR-INI-7`, `CLI-CONTRACT.md` §7.4) —
//! `pio pkg search` has no JSON output, so the library browser calls the same registry
//! endpoint the CLI itself uses. Query qualifiers (`keyword:"..."`, `framework:"..."`, etc.)
//! are space-joined with the free-text query, each wrapped in quotes.
//!
//! Grounded in a real `GET .../v3/search?query=ArduinoJson&page=1` response captured this
//! session (`tests/fixtures/pio-registry-search-real.json`, live network access, 200 OK).

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const PRIMARY_HOST: &str = "https://api.registry.platformio.org";
pub const MIRROR_HOST: &str = "https://api.registry.nm1.platformio.org";

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RegistryPackage {
    pub owner: String,
    pub name: String,
    pub r#type: String,
    pub tier: String,
    pub description: String,
    pub version: String,
    pub released_at: Option<String>,
    /// Never filled in from the search response itself — the caller cross-references
    /// `pkg_installed` for the active workspace to populate this (`IPC-CONTRACT.md` §7).
    pub installed_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PackagePage {
    pub items: Vec<RegistryPackage>,
    pub page: u32,
    pub total: u32,
    pub limit: u32,
}

#[derive(Debug, Deserialize)]
struct RawResponse {
    #[serde(default)]
    page: u32,
    #[serde(default)]
    limit: u32,
    #[serde(default)]
    total: u32,
    #[serde(default)]
    items: Vec<RawItem>,
}

#[derive(Debug, Deserialize)]
struct RawItem {
    #[serde(default)]
    owner: Option<RawOwner>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default)]
    tier: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    version: Option<RawVersion>,
}

#[derive(Debug, Deserialize)]
struct RawOwner {
    #[serde(default)]
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawVersion {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    released_at: Option<String>,
}

/// `CLI-CONTRACT.md` §7.4: qualifier syntax is `key:"value"`, space-joined with the
/// free-text query — the qualifier name is the **singular** of the filter (`keyword`, not
/// `keywords`).
pub fn build_query(free_text: &str, qualifiers: &[(String, String)]) -> String {
    let mut parts = Vec::new();
    let trimmed = free_text.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
    for (key, value) in qualifiers {
        parts.push(format!("{key}:\"{value}\""));
    }
    parts.join(" ")
}

/// Parses one host's response body — never fails on an unrecognized/missing field
/// (`CLI-CONTRACT.md` §7.4: "the full response schema is not documented; code defensively
/// and treat any missing field as absent rather than erroring"), only on genuinely
/// malformed JSON.
pub fn parse_search_response(json: &str) -> Result<PackagePage> {
    let raw: RawResponse = serde_json::from_str(json).map_err(|e| AppError::Io {
        message: format!("parsing the registry search response: {e}"),
    })?;
    let items = raw
        .items
        .into_iter()
        .filter_map(|item| {
            Some(RegistryPackage {
                owner: item.owner?.username?,
                name: item.name?,
                r#type: item.r#type.unwrap_or_else(|| "library".into()),
                tier: item.tier.unwrap_or_default(),
                description: item.description.unwrap_or_default(),
                version: item.version.as_ref().and_then(|v| v.name.clone())?,
                released_at: item.version.and_then(|v| v.released_at),
                installed_version: None,
            })
        })
        .collect();
    Ok(PackagePage {
        items,
        page: raw.page,
        total: raw.total,
        limit: raw.limit,
    })
}

/// Builds the search URL for a given host — query parameters are appended via
/// `reqwest::RequestBuilder::query`, not manual string formatting, so encoding is handled
/// by the `url` crate rather than hand-rolled.
pub async fn search(client: &reqwest::Client, query: &str, page: u32, sort: Option<&str>) -> Result<PackagePage> {
    for host in [PRIMARY_HOST, MIRROR_HOST] {
        let mut params: Vec<(&str, String)> = vec![("query", query.to_string()), ("page", page.to_string())];
        if let Some(s) = sort {
            params.push(("sort", s.to_string()));
        }
        let request = client.get(format!("{host}/v3/search")).query(&params);
        let Ok(response) = request.send().await else {
            continue; // try the mirror
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(body) = response.text().await else {
            continue;
        };
        if let Ok(page) = parse_search_response(&body) {
            return Ok(page);
        }
    }
    Err(AppError::NetworkUnavailable { host: PRIMARY_HOST.into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_captured_search_response() {
        let json = include_str!("../../../../tests/fixtures/pio-registry-search-real.json");
        let page = parse_search_response(json).expect("parse");
        assert_eq!(page.page, 1);
        assert_eq!(page.limit, 10);
        assert_eq!(page.total, 409);
        assert_eq!(page.items.len(), 10);

        let arduino_json = &page.items[0];
        assert_eq!(arduino_json.owner, "bblanchon");
        assert_eq!(arduino_json.name, "ArduinoJson");
        assert_eq!(arduino_json.r#type, "library");
        assert_eq!(arduino_json.tier, "community");
        assert_eq!(arduino_json.version, "7.4.3");
        assert_eq!(arduino_json.released_at.as_deref(), Some("2026-03-02T17:23:45Z"));
        assert_eq!(arduino_json.installed_version, None);
    }

    #[test]
    fn an_item_missing_a_required_field_is_skipped_not_fatal() {
        let json = r#"{"page":1,"limit":10,"total":1,"items":[{"type":"library","tier":"community","description":"no owner or name"}]}"#;
        let page = parse_search_response(json).expect("parse");
        assert!(page.items.is_empty());
    }

    #[test]
    fn malformed_json_is_a_typed_error() {
        assert!(parse_search_response("not json").is_err());
    }

    #[test]
    fn build_query_joins_free_text_and_qualifiers() {
        assert_eq!(
            build_query("json parser", &[("framework".to_string(), "arduino".to_string())]),
            "json parser framework:\"arduino\""
        );
        assert_eq!(build_query("", &[("owner".to_string(), "bblanchon".to_string())]), "owner:\"bblanchon\"");
        assert_eq!(build_query("json", &[]), "json");
    }
}
