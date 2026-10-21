//! 安装计划的构建：把版本 JSON 拆成一串“要下载的文件”。
//!
//! 这里只做计算：拉版本 JSON、拉 asset index，然后把该下什么算清楚。
//! 预览和真正安装共用同一份计划，避免“预览说 3 个文件、实际下 3000 个”。
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::download::file::FileTask;
use crate::download::mirror::SourceChoice;
use crate::download::plan;
use crate::error::{AppError, AppResult};
use crate::fsutil;
use crate::minecraft::classpath;
use crate::minecraft::isolate;
use crate::minecraft::manifest;
use crate::minecraft::rules::{Features, Platform};
use crate::minecraft::version_json::VersionJson;
use crate::state::AppConfig;

/// 资源文件（assets）的官方根路径。
/// BMCLAPI 的资源路径与官方不同形，这里宁可直接连官方，
/// 也不拼一个可能 404 的镜像地址（要改的话得先确认镜像的实际路由）。
const RESOURCES: &str = "https://resources.download.minecraft.net";

/// 缺失文件名字最多返回几个，避免把三千个 sha1 名字全塞进 IPC
const MAX_MISSING_NAMES: usize = 20;

#[derive(Default)]
pub struct Planned {
    pub client: Option<FileTask>,
    pub libraries: Vec<FileTask>,
    pub assets: Vec<FileTask>,
    pub logging: Option<FileTask>,
}

impl Planned {
    /// (客户端, 依赖库, 资源文件)
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            if self.client.is_some() { 1 } else { 0 },
            self.libraries.len(),
            self.assets.len(),
        )
    }

    pub fn file_count(&self) -> usize {
        let (c, l, a) = self.counts();
        c + l + a + if self.logging.is_some() { 1 } else { 0 }
    }

    pub fn total_bytes(&self) -> u64 {
        let mut sum = 0u64;
        for t in self.libraries.iter().chain(self.assets.iter()) {
            sum += t.size;
        }
        if let Some(t) = &self.client {
            sum += t.size;
        }
        if let Some(t) = &self.logging {
            sum += t.size;
        }
        sum
    }
}

/// 缺失统计：把「装没装」从单个布尔升级成三态判断的依据。
/// assets 动辄三千个，所以只留前 20 个名字给界面提示，数量仍然精确。
#[derive(Default)]
pub struct Missing {
    pub client: bool,
    pub libraries: usize,
    pub assets: usize,
    pub logging: bool,
    pub names: Vec<String>,
}

impl Missing {
    pub fn count(&self) -> usize {
        (if self.client { 1 } else { 0 })
            + self.libraries
            + self.assets
            + (if self.logging { 1 } else { 0 })
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// 客户端 jar 在 → 至少是可以启动的版本，缺的只是依赖库和资源
    pub fn status(&self) -> &'static str {
        if self.client {
            "missing"
        } else if self.is_empty() {
            "complete"
        } else {
            "partial"
        }
    }
}

/// 逐项比对磁盘，找出还缺哪些文件。
/// 只用 `is_file()`，不校验 sha1：预览阶段读几千个文件的哈希会卡几十秒。
impl Planned {
    pub fn missing_required(&self) -> Missing {
        let mut m = Missing::default();
        if let Some(t) = &self.client {
            if !t.dest.is_file() {
                m.client = true;
                note_missing(&mut m.names, t);
            }
        }
        for t in &self.libraries {
            if !t.dest.is_file() {
                m.libraries += 1;
                note_missing(&mut m.names, t);
            }
        }
        for t in &self.assets {
            if !t.dest.is_file() {
                m.assets += 1;
                note_missing(&mut m.names, t);
            }
        }
        if let Some(t) = &self.logging {
            if !t.dest.is_file() {
                m.logging = true;
                note_missing(&mut m.names, t);
            }
        }
        m
    }
}

fn note_missing(names: &mut Vec<String>, task: &FileTask) {
    if names.len() >= MAX_MISSING_NAMES {
        return;
    }
    names.push(
        task.dest
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| task.dest.display().to_string()),
    );
}

/// 单文件超时：client.jar 和 assets 里都有几十 MB 的东西，
/// 用设置里那个 4 秒的镜像超时会全部失败，至少给 60 秒。
pub fn timeout_of(cfg: &AppConfig) -> Duration {
    Duration::from_secs(cfg.settings.mirror_timeout_secs.max(3).saturating_mul(5).max(60))
}

pub fn choice_of(cfg: &AppConfig) -> SourceChoice {
    if cfg.settings.prefer_mirror {
        SourceChoice::MirrorOnly
    } else {
        SourceChoice::OfficialFirst
    }
}

/// 从清单找到版本，再拉它的 JSON。返回 (解析结果, 原始文本)。
pub fn fetch_version_json(
    client: &reqwest::blocking::Client,
    id: &str,
    cfg: &AppConfig,
) -> AppResult<(VersionJson, String)> {
    let (m, _url) = manifest::fetch(client, choice_of(cfg), timeout_of(cfg))?;
    let entry = m
        .versions
        .iter()
        .find(|v| v.id == id)
        .ok_or_else(|| AppError::VersionInvalid(format!("官方清单里没有 {id}，可能已下架")))?;
    let got = plan::fetch_text(client, &entry.url, choice_of(cfg), timeout_of(cfg))?;
    let version: VersionJson = serde_json::from_str(&got.body)?;
    Ok((version, got.body))
}

/// 先写 `.part` 再改名：中途崩了也不会留下半截 JSON 让启动器以为装好了
pub fn save_version_json(root: &Path, id: &str, body: &str) -> AppResult<()> {
    let dir = isolate::version_dir(root, id);
    fsutil::ensure_dir(&dir)?;
    let target = isolate::json_path(root, id);
    let tmp = target.with_file_name(format!("{id}.json.part"));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &target)?;
    Ok(())
}

#[derive(Deserialize)]
struct AssetIndexBody {
    #[serde(default)]
    objects: HashMap<String, AssetObject>,
}

#[derive(Deserialize)]
struct AssetObject {
    hash: String,
    #[serde(default)]
    size: u64,
}

pub fn build(
    client: &reqwest::blocking::Client,
    root: &Path,
    id: &str,
    cfg: &AppConfig,
    version: &VersionJson,
    save_index: bool,
) -> AppResult<Planned> {
    let platform = Platform::current();
    let features = Features {
        demo: false,
        custom_resolution: true,
        quick_play: false,
    };
    let libs_dir = isolate::libraries_dir(root);

    // client.jar：没有下载信息就没什么可装的了
    let client_task = {
        let art = version
            .downloads
            .as_ref()
            .and_then(|d| d.client.as_ref())
            .ok_or_else(|| AppError::VersionInvalid("这个版本没有客户端下载信息".into()))?;
        let url = art
            .url
            .clone()
            .ok_or_else(|| AppError::VersionInvalid("客户端下载地址缺失".into()))?;
        Some(FileTask::new(
            url,
            isolate::jar_path(root, id),
            art.sha1.clone().unwrap_or_default(),
            art.size.unwrap_or(0),
        ))
    };

    // 依赖库：classpath::collect 已经把 rules 求值和 natives 分类器处理过了，
    // 这里只把 path/url/sha1 拼成下载任务
    let mut libraries = Vec::new();
    for e in classpath::collect(version, &platform, &features) {
        let (Some(url), Some(sha1)) = (e.url.clone(), e.sha1.clone()) else {
            continue;
        };
        libraries.push(FileTask::new(
            url,
            libs_dir.join(&e.path),
            sha1,
            e.size.unwrap_or(0),
        ));
    }

    let mut assets = Vec::new();
    if let Some(idx) = version.asset_index.as_ref() {
        if let Some(url) = idx.url.clone() {
            let got = plan::fetch_text(client, &url, choice_of(cfg), timeout_of(cfg))?;
            let body: AssetIndexBody = serde_json::from_str(&got.body)?;
            // 预览时只想知道有多少个资源文件，不必真往盘上写
            if save_index {
                let indexes = isolate::assets_dir(root).join("indexes");
                fsutil::ensure_dir(&indexes)?;
                std::fs::write(indexes.join(format!("{}.json", idx.id)), &got.body)?;
            }

            let objects = isolate::assets_dir(root).join("objects");
            for obj in body.objects.into_values() {
                let Some(prefix) = obj.hash.get(0..2).map(|s| s.to_string()) else {
                    continue;
                };
                let dest = objects.join(&prefix).join(&obj.hash);
                assets.push(FileTask::new(
                    format!("{RESOURCES}/{prefix}/{}", obj.hash),
                    dest,
                    obj.hash.clone(),
                    obj.size,
                ));
            }
        }
    }

    // log4j 配置：文件名优先用 id，官方有时只给 url
    let mut logging = None;
    if let Some(f) = version
        .logging
        .as_ref()
        .and_then(|l| l.client.as_ref())
        .and_then(|c| c.file.as_ref())
    {
        if let Some(url) = f.url.clone() {
            let name = f
                .id
                .clone()
                .or_else(|| url.rsplit('/').next().map(|s| s.to_string()))
                .ok_or_else(|| AppError::Message("日志配置文件名缺失".into()))?;
            logging = Some(FileTask::new(
                url,
                isolate::assets_dir(root).join("log_configs").join(name),
                f.sha1.clone().unwrap_or_default(),
                f.size.unwrap_or(0),
            ));
        }
    }

    Ok(Planned {
        client: client_task,
        libraries,
        assets,
        logging,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::file::FileTask;

    fn task(size: u64) -> FileTask {
        FileTask::new("http://x/f", "/tmp/f".into(), "", size)
    }

    #[test]
    fn timeout_is_at_least_a_minute() {
        // 设置里默认 4 秒，乘 5 只有 20 秒，必须抬到 60 秒下限
        let cfg = AppConfig::default();
        assert_eq!(timeout_of(&cfg).as_secs(), 60);
        let mut slow = AppConfig::default();
        slow.settings.mirror_timeout_secs = 30;
        assert_eq!(timeout_of(&slow).as_secs(), 150);
    }

    #[test]
    fn totals_add_every_part() {
        let mut p = Planned::default();
        p.client = Some(task(100));
        p.libraries = vec![task(10), task(20)];
        p.assets = vec![task(1000)];
        p.logging = Some(task(5));
        assert_eq!(p.counts(), (1, 2, 1));
        assert_eq!(p.file_count(), 5);
        assert_eq!(p.total_bytes(), 1135);
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nova-plan-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_required_gives_three_states() {
        let dir = temp_dir("missing");
        let have = dir.join("have.jar");
        std::fs::write(&have, b"x").unwrap();

        // 客户端缺 → missing
        let mut p = Planned::default();
        p.client = Some(FileTask::new("http://x/c", dir.join("client.jar"), "", 1));
        p.libraries = vec![
            FileTask::new("http://x/l1", have.clone(), "", 1),
            FileTask::new("http://x/l2", dir.join("lib2.jar"), "", 1),
        ];
        p.assets = vec![FileTask::new("http://x/a", dir.join("a1"), "", 1)];
        let m = p.missing_required();
        assert!(m.client);
        assert_eq!(m.libraries, 1);
        assert_eq!(m.assets, 1);
        assert_eq!(m.count(), 3);
        assert_eq!(m.status(), "missing");
        // 磁盘上已有的不进缺失名单
        assert!(m.names.contains(&"lib2.jar".to_string()));
        assert!(!m.names.contains(&"have.jar".to_string()));

        // 全齐 → complete
        p.client = Some(FileTask::new("http://x/c", have.clone(), "", 1));
        p.libraries.clear();
        p.assets.clear();
        let m2 = p.missing_required();
        assert_eq!(m2.count(), 0);
        assert_eq!(m2.status(), "complete");

        // 客户端在、依赖缺 → partial
        p.libraries = vec![FileTask::new("http://x/l", dir.join("nope.jar"), "", 1)];
        let m3 = p.missing_required();
        assert!(!m3.client);
        assert_eq!(m3.status(), "partial");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_names_are_capped() {
        // 三千个 assets 全缺时不能把名字全塞进 IPC
        let dir = temp_dir("cap");
        let mut p = Planned::default();
        p.assets = (0..40)
            .map(|i| FileTask::new("http://x/a", dir.join(format!("a{i}")), "", 1))
            .collect();
        let m = p.missing_required();
        assert_eq!(m.assets, 40);
        assert_eq!(m.names.len(), MAX_MISSING_NAMES);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
