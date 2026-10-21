//! 整合包导入执行：解压 overrides、下载索引里列的文件、写实例元数据。
//!
//! 实例目录是 `instances/<id>/`，和 `versions/<id>/` 平级：
//! 版本目录放「怎么启动」（版本 JSON、加载器、libraries），
//! 实例目录放「这一套 Mod 和配置」，启动时把 gameDir 指过去即可。
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::download::file::{self, FileTask};
use crate::error::{AppError, AppResult};
use crate::fsutil;
use crate::loader::meta::LoaderKind;

use super::mrpack::{self, PackFile};

/// 导入过程中回传的事件，和加载器安装那套保持一致的心智模型
pub enum Event {
    Stage(String),
    Progress { done: u64, total: u64 },
    Log(String),
}

/// 预览阶段算出来的安装计划：不落盘、不联网
#[derive(Debug, Clone)]
pub struct Plan {
    pub id: String,
    /// 去掉重名后缀的原始 id，预览用它判断会不会另存为 `-2`
    pub base_id: String,
    pub name: String,
    pub summary: String,
    pub game: String,
    pub loader: Option<(LoaderKind, String)>,
    pub dir: PathBuf,
    pub files: Vec<PackFile>,
    /// 索引里声明的总体积，进度条的分母
    pub total: u64,
    /// 源包路径，解压 overrides 时还要再打开一次
    pub pack: PathBuf,
}

/// 磁盘上的实例元数据。前端列表直接读它
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    pub game: String,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub loader_version: Option<String>,
    /// 启动时要用的版本目录名（加载器版本或原版），加载器没装成时是 None
    #[serde(default)]
    pub launch_id: Option<String>,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub file_count: usize,
    /// 上次导入没下下来的条目（实例内相对路径），空表示这次是全的
    #[serde(default)]
    pub failed: Vec<String>,
    /// 源整合包路径，重试缺失文件时还要再读一次索引
    #[serde(default)]
    pub source: Option<String>,
}

pub struct ImportReport {
    pub instance: Instance,
    pub downloaded: usize,
    pub skipped: usize,
    /// 下载失败的条目，导入不整体失败，只把这些列出来
    pub failed: Vec<String>,
    pub overrides: usize,
}

pub fn instances_dir(root: &Path) -> PathBuf {
    root.join("instances")
}

pub fn instance_dir(root: &Path, id: &str) -> PathBuf {
    instances_dir(root).join(id)
}

fn instance_json(root: &Path, id: &str) -> PathBuf {
    instance_dir(root, id).join("instance.json")
}

/// 预览：只读索引，算好实例 id（重名自动加后缀）与要下的文件
pub fn plan(root: &Path, pack: &Path) -> AppResult<Plan> {
    let index = mrpack::read_index(pack)?;
    let stem = pack
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = mrpack::instance_id(&index.name, &stem);
    let mut id = base.clone();
    let mut n = 2;
    while instance_dir(root, &id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }

    let files = index.client_files();
    let total = files.iter().map(|f| f.file_size).sum();
    Ok(Plan {
        dir: instance_dir(root, &id),
        id,
        base_id: base,
        name: if index.name.trim().is_empty() {
            stem
        } else {
            index.name.clone()
        },
        summary: index.summary.clone(),
        game: index.game_version().to_string(),
        loader: index.loader(),
        files,
        total,
        pack: pack.to_path_buf(),
    })
}

/// 单个文件解压上限：整合包里的 Mod / 配置基本都在几百 MB 以内
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
/// 一次导入的总解压上限：挡 zip 炸弹
const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// 条目数上限：正常整合包几千个文件，两万已经很宽松
const MAX_ENTRIES: usize = 20_000;

/// 解压配额检查
///
/// 抽成纯函数是为了能单测：真造一个 2GB 的 zip 来测不现实
fn check_quota(entries: usize, entry_bytes: u64, total_bytes: u64) -> AppResult<()> {
    if entries > MAX_ENTRIES {
        return Err(AppError::Message(
            "整合包文件数超出安全上限，已中止解压".into(),
        ));
    }
    if entry_bytes > MAX_ENTRY_BYTES {
        return Err(AppError::Message(
            "整合包里有个单文件超出安全上限，已中止解压".into(),
        ));
    }
    if total_bytes > MAX_TOTAL_BYTES {
        return Err(AppError::Message(
            "整合包解压总量超出安全上限，已中止解压".into(),
        ));
    }
    Ok(())
}

/// 把一个 `prefix/` 目录下的文件铺到 `dest`，返回写出的文件数。
/// 名字先过 `safe_rel_path`：zip 里的路径和索引里的路径一样不可信
fn extract_dir(pack: &Path, dest: &Path, prefix: &str, emit: &mut dyn FnMut(Event)) -> AppResult<usize> {
    let file = std::fs::File::open(pack)
        .map_err(|e| AppError::Message(format!("打不开整合包：{e}")))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::Message(format!("整合包已损坏：{e}")))?;
    let head = format!("{prefix}/");
    let mut written = 0usize;
    let mut total_bytes: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = match zip.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let Some(rel) = name.strip_prefix(&head) else {
            continue;
        };
        let Ok(rel) = mrpack::safe_rel_path(rel) else {
            emit(Event::Log(format!("跳过可疑路径：{name}")));
            continue;
        };
        // 写下之前先按 zip 头部声明的大小卡一道，能挡掉大部分炸弹
        check_quota(written + 1, entry.size(), total_bytes)?;
        let out = dest.join(&rel);
        if let Some(parent) = out.parent() {
            fsutil::ensure_dir(parent)?;
        }
        let mut f = std::fs::File::create(&out)
            .map_err(|e| AppError::Message(format!("写入 {} 失败：{e}", out.display())))?;
        // 实际写出的字节才是可信的：entry.size() 只是 zip 头里的声明
        let copied = std::io::copy(&mut entry, &mut f)
            .map_err(|e| AppError::Message(format!("写入 {} 失败：{e}", out.display())))?;
        f.flush().ok();
        total_bytes += copied;
        written += 1;
        check_quota(written, 0, total_bytes)?;
    }
    Ok(written)
}

/// 按索引下载文件。单个文件失败只记账不中断 —— 一个失效的 CDN 链接
/// 不该让整包导入白做一遍
fn download_files(
    client: &reqwest::blocking::Client,
    dir: &Path,
    files: &[PackFile],
    total: u64,
    timeout: Duration,
    emit: &mut dyn FnMut(Event),
) -> (usize, usize, Vec<String>) {
    let mut downloaded = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<String> = Vec::new();
    let mut done = 0u64;

    for f in files {
        let (Ok(rel), Some(url)) = (mrpack::safe_rel_path(&f.path), f.download_url()) else {
            failed.push(f.path.clone());
            continue;
        };
        let dest = dir.join(&rel);
        if let Some(parent) = dest.parent() {
            if fsutil::ensure_dir(parent).is_err() {
                failed.push(rel);
                continue;
            }
        }
        // 整合包索引常常只给 SHA512，两个都传下去：下载器会优先用 SHA512
        let task = FileTask::new(url, dest, f.hashes.sha1.clone(), f.file_size)
            .with_sha512(f.hashes.sha512.clone());
        if file::already_ok(&task) {
            skipped += 1;
            done += f.file_size;
            emit(Event::Progress { done, total });
            continue;
        }
        match file::download_file(client, &task, timeout, &mut |_, _| {}) {
            Ok(()) => {
                downloaded += 1;
                done += f.file_size;
                emit(Event::Progress { done, total });
            }
            Err(e) => {
                emit(Event::Log(format!("{rel} 下载失败：{e}")));
                done += f.file_size;
                emit(Event::Progress { done, total });
                failed.push(rel);
            }
        }
    }
    (downloaded, skipped, failed)
}

/// 真正导入：解压 → 下载 → 落元数据
pub fn import(
    client: &reqwest::blocking::Client,
    root: &Path,
    plan: &Plan,
    timeout: Duration,
    emit: &mut dyn FnMut(Event),
) -> AppResult<ImportReport> {
    fsutil::ensure_dir(&plan.dir)?;

    emit(Event::Stage("正在解压覆盖文件".into()));
    // 先铺 overrides，再铺 client-overrides：后者要能覆盖前者
    // &mut dyn 不是 Copy，同一个 emit 连续用两次要显式 reborrow，否则第二次就是「use of moved value」
    let mut overrides = extract_dir(&plan.pack, &plan.dir, mrpack::OVERRIDES, &mut *emit)?;
    overrides += extract_dir(&plan.pack, &plan.dir, mrpack::CLIENT_OVERRIDES, &mut *emit)?;
    emit(Event::Log(format!("覆盖文件解压完成，共 {overrides} 个")));

    emit(Event::Stage("正在下载整合包文件".into()));
    let (downloaded, skipped, failed) =
        download_files(client, &plan.dir, &plan.files, plan.total, timeout, emit);

    let now = now_secs();
    let instance = Instance {
        id: plan.id.clone(),
        name: plan.name.clone(),
        summary: plan.summary.clone(),
        game: plan.game.clone(),
        loader: plan.loader.as_ref().map(|(k, _)| k.key().to_string()),
        loader_version: plan.loader.as_ref().map(|(_, v)| v.clone()),
        launch_id: None,
        created: now,
        file_count: plan.files.len(),
        failed: failed.clone(),
        source: Some(plan.pack.to_string_lossy().into_owned()),
    };
    write_instance(root, &instance)?;

    if !failed.is_empty() {
        emit(Event::Log(format!(
            "有 {} 个文件没下下来，游戏里可能会缺东西",
            failed.len()
        )));
    }
    Ok(ImportReport {
        instance,
        downloaded,
        skipped,
        failed,
        overrides,
    })
}

/// 重试缺失文件：只补 `instance.failed` 里那些，不重解压 overrides。
/// 返回补完之后的实例（`failed` 已更新，可能仍然非空）
pub fn retry(
    client: &reqwest::blocking::Client,
    root: &Path,
    id: &str,
    timeout: Duration,
    emit: &mut dyn FnMut(Event),
) -> AppResult<Instance> {
    let mut instance = read_instance(root, id)
        .ok_or_else(|| AppError::Message(format!("找不到实例 {id}")))?;

    let source = instance
        .source
        .clone()
        .ok_or_else(|| AppError::Message("这个实例不是从整合包导入的，没有源包可重试".into()))?;
    let pack = PathBuf::from(&source);
    if !pack.is_file() {
        return Err(AppError::Message(format!(
            "源整合包不在了（可能已被移动或删除）：{source}"
        )));
    }

    // 只重下上次失败的那些；清单可能变过，所以按路径重新对一遍
    let index = mrpack::read_index(&pack)?;
    let failed_set: Vec<String> = instance
        .failed
        .iter()
        .filter_map(|p| mrpack::safe_rel_path(p).ok())
        .collect();
    let mut want: Vec<PackFile> = index.client_files();
    if !failed_set.is_empty() {
        want.retain(|f| match mrpack::safe_rel_path(&f.path) {
            Ok(rel) => failed_set.contains(&rel),
            Err(_) => false,
        });
    }
    if want.is_empty() {
        emit(Event::Log("没有需要重试的文件".into()));
        return Ok(instance);
    }

    let total = want.iter().map(|f| f.file_size).sum();
    emit(Event::Stage(format!("正在重试 {} 个文件", want.len())));
    let dir = instance_dir(root, id);
    let (_downloaded, _skipped, failed) = download_files(client, &dir, &want, total, timeout, emit);

    instance.failed = failed;
    write_instance(root, &instance)?;
    if instance.failed.is_empty() {
        emit(Event::Log("缺失文件已经全部补齐".into()));
    } else {
        emit(Event::Log(format!(
            "还有 {} 个文件没下下来",
            instance.failed.len()
        )));
    }
    Ok(instance)
}

/// 创建时间存 Unix 秒：不引 chrono，秒数交给前端格式化成本更低
fn now_secs() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn write_instance(root: &Path, instance: &Instance) -> AppResult<()> {
    let dir = instance_dir(root, &instance.id);
    fsutil::ensure_dir(&dir)?;
    // 走 fsutil 的原子写：先写临时文件再改名，断电不会留半个 json
    fsutil::write_json_atomic(&instance_json(root, &instance.id), instance)
}

/// 读一个实例的元数据；坏掉的目录直接跳过，不让它把整个列表带崩
pub fn read_instance(root: &Path, id: &str) -> Option<Instance> {
    fsutil::read_json::<Instance>(&instance_json(root, id))
        .ok()
        .flatten()
}

/// 把加载器版本回填进实例：装完加载器才能启动
pub fn set_launch_id(root: &Path, id: &str, launch_id: &str) -> AppResult<()> {
    let mut instance = read_instance(root, id)
        .ok_or_else(|| AppError::Message(format!("找不到实例 {id}")))?;
    instance.launch_id = Some(launch_id.to_string());
    write_instance(root, &instance)
}

pub fn list(root: &Path) -> Vec<Instance> {
    let dir = instances_dir(root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<Instance> = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if let Some(instance) = read_instance(root, &id) {
            out.push(instance);
        }
    }
    // 新的排前面：时间戳是 Unix 秒，字符串比较在这长度下也不会乱
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

pub fn remove(root: &Path, id: &str) -> AppResult<()> {
    // id 来自界面，同样不能直接拼
    let id = mrpack::safe_rel_path(id)?;
    if id.contains('/') {
        return Err(AppError::Message(format!("实例名不合法：{id}")));
    }
    let dir = instance_dir(root, &id);
    if !dir.is_dir() {
        return Err(AppError::Message(format!("找不到实例 {id}")));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| AppError::Message(format!("删除实例失败：{e}")))
}

/// 给测试用：把一段 json 当索引写进临时 zip 太麻烦，
/// 这里只测不依赖文件的纯逻辑（路径白名单、id 生成在 mrpack 里）
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_dir_layout() {
        let root = Path::new("/mc");
        assert_eq!(instance_dir(root, "pack"), Path::new("/mc/instances/pack"));
        assert_eq!(instances_dir(root), Path::new("/mc/instances"));
    }

    #[test]
    fn remove_rejects_traversal() {
        let root = Path::new("/mc");
        assert!(remove(root, "../etc").is_err());
        assert!(remove(root, "a/b").is_err());
    }

    #[test]
    fn read_missing_instance_is_none() {
        let root = Path::new("/mc");
        assert!(read_instance(root, "nobody").is_none());
    }

    #[test]
    fn quota_rejects_too_many_entries() {
        assert!(check_quota(MAX_ENTRIES, 0, 0).is_ok());
        assert!(check_quota(MAX_ENTRIES + 1, 0, 0).is_err());
    }

    #[test]
    fn quota_rejects_oversized_entry() {
        assert!(check_quota(1, MAX_ENTRY_BYTES, 0).is_ok());
        assert!(check_quota(1, MAX_ENTRY_BYTES + 1, 0).is_err());
    }

    #[test]
    fn quota_rejects_oversized_total() {
        assert!(check_quota(1, 0, MAX_TOTAL_BYTES).is_ok());
        assert!(check_quota(1, 0, MAX_TOTAL_BYTES + 1).is_err());
    }

    #[test]
    fn retry_requires_source_and_existing_instance() {
        let root = std::env::temp_dir().join(format!("nova-modpack-{}", uuid::Uuid::new_v4()));
        let inst = Instance {
            id: "p".into(),
            name: "p".into(),
            summary: String::new(),
            game: "1.20.1".into(),
            loader: None,
            loader_version: None,
            launch_id: None,
            created: "0".into(),
            file_count: 0,
            failed: Vec::new(),
            source: None,
        };
        write_instance(&root, &inst).unwrap();

        let client = reqwest::blocking::Client::new();
        let mut emit = |_e: Event| {};
        // 没有源包路径 → 明确报错，而不是静默当成“没有要补的”
        assert!(retry(&client, &root, "p", Duration::from_secs(1), &mut emit).is_err());
        // 实例都不存在
        assert!(retry(&client, &root, "nope", Duration::from_secs(1), &mut emit).is_err());

        let _ = std::fs::remove_dir_all(&root);
    }
}