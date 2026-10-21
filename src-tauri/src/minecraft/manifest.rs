//! 版本清单（version_manifest_v2.json）的拉取与结构。
//! 注意：清单只给版本 id 和每个版本的 JSON 地址，
//! 真正的下载地址要拿 version.url 再拉一次，别在这里猜。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::download::mirror::SourceChoice;
use crate::download::plan;
use crate::error::AppResult;

pub const MANIFEST_URL: &str = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: Latest,
    #[serde(default)]
    pub versions: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Latest {
    #[serde(default)]
    pub release: String,
    #[serde(default)]
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub time: String,
    #[serde(rename = "releaseTime", default)]
    pub release_time: String,
}

/// 拉清单。返回 (清单, 实际生效的 URL)，URL 含 bmclapi 就说明走了镜像。
pub fn fetch(
    client: &reqwest::blocking::Client,
    choice: SourceChoice,
    timeout: Duration,
) -> AppResult<(VersionManifest, String)> {
    plan::fetch_json::<VersionManifest>(client, MANIFEST_URL, choice, timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_manifest() {
        let raw = r#"{
            "latest": { "release": "26.3", "snapshot": "26.4-snapshot-5" },
            "versions": [
                { "id": "26.3", "type": "release", "url": "https://piston-meta.mojang.com/v1/packages/x/26.3.json", "time": "t", "releaseTime": "2026-09-15T00:00:00+00:00" }
            ]
        }"#;
        let m: VersionManifest = serde_json::from_str(raw).unwrap();
        assert_eq!(m.latest.release, "26.3");
        assert_eq!(m.versions.len(), 1);
        assert_eq!(m.versions[0].kind, "release");
        assert!(m.versions[0].release_time.starts_with("2026-09-15"));
    }
}
