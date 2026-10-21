//! 单文件下载：流式写盘 + 摘要校验 + 已就位则跳过。
//!
//! 下载引擎的最小单元。安装 Java 运行时、补全 libraries、下载 client.jar
//! 都走这一条路径，避免各写一套超时与校验逻辑。
//!
//! 摘要同时支持 SHA1 与 SHA512：Minecraft 官方清单给 SHA1，Modrinth 的
//! 整合包索引给 SHA512（偶尔只有它），用哪个由调用方填。

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::fsutil;

/// 一个待下载的文件
#[derive(Debug, Clone)]
pub struct FileTask {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: String,
    /// 只有整合包索引这类来源会给；空字符串表示没有
    pub sha512: String,
    pub size: u64,
}

impl FileTask {
    /// 保持四参数签名：绝大多数调用方只有 SHA1，不用关心 SHA512
    pub fn new(url: impl Into<String>, dest: PathBuf, sha1: impl Into<String>, size: u64) -> Self {
        Self {
            url: url.into(),
            dest,
            sha1: sha1.into(),
            sha512: String::new(),
            size,
        }
    }

    /// 链式补 SHA512：`FileTask::new(..).with_sha512(x)`
    pub fn with_sha512(mut self, sha512: impl Into<String>) -> Self {
        self.sha512 = sha512.into();
        self
    }

    /// 有没有任何一个可用于校验的摘要
    pub fn has_digest(&self) -> bool {
        !self.sha512.is_empty() || !self.sha1.is_empty()
    }
}

/// 已经就位：存在 + 大小对得上 + 摘要一致
///
/// 优先用 SHA512：两者都在时它更强，而整合包常常只给 SHA512。
/// 一个摘要都没有时退回比大小；连大小都不知道（=0）就不认，
/// 免得把上次中断留下的空壳当成已经下好了。
pub fn already_ok(task: &FileTask) -> bool {
    if !task.dest.is_file() {
        return false;
    }
    if task.size > 0 {
        match fs::metadata(&task.dest) {
            Ok(m) if m.len() == task.size => {}
            _ => return false,
        }
    } else if !task.has_digest() {
        return false;
    }

    if !task.sha512.is_empty() {
        return match fsutil::sha512_file(&task.dest) {
            Ok(v) => v.eq_ignore_ascii_case(&task.sha512),
            Err(_) => false,
        };
    }
    if !task.sha1.is_empty() {
        return match fsutil::sha1_file(&task.dest) {
            Ok(v) => v.eq_ignore_ascii_case(&task.sha1),
            Err(_) => false,
        };
    }
    true
}

/// 临时文件名：`java.exe` -> `java.exe.part`，避免不同扩展名撞车
fn temp_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".to_string());
    dest.with_file_name(format!("{name}.part"))
}

/// 已有的 `.part` 还能续写多少：为空、已够长、或读不到都返回 0（当作从头下）
fn resume_offset(tmp: &Path, size: u64) -> u64 {
    let Ok(meta) = fs::metadata(tmp) else {
        return 0;
    };
    if !meta.is_file() {
        return 0;
    }
    let len = meta.len();
    if len == 0 {
        return 0;
    }
    // 比清单声明的还长，说明这份临时文件不可信，直接重下
    if size > 0 && len >= size {
        return 0;
    }
    len
}

pub fn download_file(
    client: &reqwest::blocking::Client,
    task: &FileTask,
    timeout: Duration,
    progress: &mut dyn FnMut(u64, u64),
) -> AppResult<()> {
    if already_ok(task) {
        progress(task.size, task.size);
        return Ok(());
    }
    if let Some(parent) = task.dest.parent() {
        fsutil::ensure_dir(parent)?;
    }

    let tmp = temp_path(&task.dest);
    // 最多两轮：第一轮可能续写失败，第二轮从零重来；还不行就报错
    for round in 1..=2u8 {
        // 第二轮不再续写，避免拿到同样残缺的字节
        let have = if round == 1 { resume_offset(&tmp, task.size) } else { 0 };

        let mut req = client.get(&task.url).timeout(timeout);
        if have > 0 {
            req = req.header("Range", format!("bytes={have}-"));
        }
        let mut resp = req.send()?;
        let status = resp.status();

        // 416：服务器不认这个 Range（临时文件长度超了），删掉从头来
        if status.as_u16() == 416 {
            let _ = fs::remove_file(&tmp);
            if round == 2 {
                return Err(AppError::Message(format!(
                    "服务器拒绝了断点续传请求：{}",
                    task.url
                )));
            }
            continue;
        }
        if !status.is_success() {
            return Err(AppError::Message(format!(
                "下载失败（HTTP {}）：{}",
                status.as_u16(),
                task.url
            )));
        }

        // 只有真的 206 才是续写；服务器忽略 Range 返回 200 时必须截断重写
        let resumed = have > 0 && status.as_u16() == 206;
        let body_len = resp.content_length().unwrap_or(0);
        let total = if resumed { have + body_len } else { body_len }.max(task.size);

        let (mut out, mut done) = if resumed {
            (OpenOptions::new().append(true).open(&tmp)?, have)
        } else {
            (File::create(&tmp)?, 0u64)
        };

        let mut buf = vec![0u8; 64 * 1024];
        let mut broken = false;
        loop {
            let n = match resp.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    // 网络中途断掉：保留 .part，下一轮接着下
                    let _ = out.sync_all();
                    if round == 1 {
                        broken = true;
                        break;
                    }
                    return Err(AppError::from(e));
                }
            };
            if n == 0 {
                break;
            }
            if let Err(e) = out.write_all(&buf[..n]) {
                return Err(AppError::from(e));
            }
            done += n as u64;
            progress(done, total.max(done));
        }
        if broken {
            continue;
        }
        out.sync_all()?;
        drop(out);

        // 先比字节数：CDN 提前掐断连接时，content-length 也会跟着变小，
        // 只看它会被骗过去，而清单里声明的大小才是准的
        if task.size > 0 && done != task.size {
            // 少了就留着 .part 再续一次，多了说明这份文件不可信
            if round == 1 && done < task.size {
                continue;
            }
            let _ = fs::remove_file(&tmp);
            return Err(AppError::Message(format!(
                "下载不完整：{}（期望 {} 字节，实际 {} 字节）",
                task.dest.display(),
                task.size,
                done
            )));
        }

        if !task.sha512.is_empty() {
            let ok = fsutil::sha512_file(&tmp)?.eq_ignore_ascii_case(&task.sha512);
            if !ok {
                let _ = fs::remove_file(&tmp);
                if round == 1 {
                    continue;
                }
                return Err(AppError::Message(format!(
                    "文件校验失败（SHA512），可能没下载完整：{}",
                    task.dest.display()
                )));
            }
        } else if !task.sha1.is_empty() {
            let ok = fsutil::sha1_file(&tmp)?.eq_ignore_ascii_case(&task.sha1);
            if !ok {
                let _ = fs::remove_file(&tmp);
                if round == 1 {
                    continue;
                }
                return Err(AppError::Message(format!(
                    "文件校验失败，可能没下载完整：{}",
                    task.dest.display()
                )));
            }
        }

        // 先删旧文件再改名，否则 Windows 上覆盖会失败
        if task.dest.exists() {
            let _ = fs::remove_file(&task.dest);
        }
        fs::rename(&tmp, &task.dest)?;
        return Ok(());
    }

    Err(AppError::Message(format!(
        "下载失败，已重试：{}",
        task.dest.display()
    )))
}

/// 清掉很久以前留下的 `.part`：取消或崩溃会攒出一堆占地方的半成品
pub fn cleanup_stale_parts(dir: &Path, max_age: Duration) -> usize {
    let mut removed = 0usize;
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            removed += cleanup_stale_parts(&path, max_age);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("part") {
            continue;
        }
        let stale = meta
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|age| age >= max_age)
            .unwrap_or(false);
        if stale && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nova-file-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn already_ok_detects_matching_file() {
        let dir = temp_dir();
        let dest = dir.join("a.bin");
        {
            let mut f = File::create(&dest).unwrap();
            f.write_all(b"hello").unwrap();
            f.sync_all().unwrap();
        }
        let sha = fsutil::sha1_file(&dest).unwrap();
        let task = FileTask::new("http://example.invalid/x", dest.clone(), sha, 5);
        assert!(already_ok(&task));

        let bad = FileTask {
            sha1: "0".repeat(40),
            ..task.clone()
        };
        assert!(!already_ok(&bad));

        let wrong_size = FileTask {
            size: 99,
            ..task.clone()
        };
        assert!(!already_ok(&wrong_size));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha512_is_preferred_and_checked() {
        let dir = temp_dir();
        let dest = dir.join("b.bin");
        {
            let mut f = File::create(&dest).unwrap();
            f.write_all(b"hello").unwrap();
            f.sync_all().unwrap();
        }
        let s512 = fsutil::sha512_file(&dest).unwrap();
        assert_eq!(s512.len(), 128);

        // 只给 sha512：要能认出来
        let task = FileTask::new("http://example.invalid/x", dest.clone(), "", 5)
            .with_sha512(s512.clone());
        assert!(already_ok(&task));

        // sha512 错了，即使 sha1 是对的也不能算通过
        let bad = task.clone().with_sha512("0".repeat(128));
        assert!(!already_ok(&bad));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_digest_and_no_size_is_not_ok() {
        let dir = temp_dir();
        let dest = dir.join("c.bin");
        {
            let mut f = File::create(&dest).unwrap();
            f.write_all(b"").unwrap();
            f.sync_all().unwrap();
        }
        // 空文件 + 没有摘要 + 不知道大小：不能当成下好了
        let task = FileTask::new("http://example.invalid/x", dest.clone(), "", 0);
        assert!(!already_ok(&task));

        // 有摘要时，即使大小未知也要校验
        let with_digest = task.clone().with_sha512(fsutil::sha512_file(&dest).unwrap());
        assert!(already_ok(&with_digest));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn has_digest_reports_either_algorithm() {
        let none = FileTask::new("http://x/y", PathBuf::from("/tmp/y"), "", 1);
        assert!(!none.has_digest());

        let one = FileTask::new("http://x/y", PathBuf::from("/tmp/y"), "abc", 1);
        assert!(one.has_digest());

        let five = FileTask::new("http://x/y", PathBuf::from("/tmp/y"), "", 1).with_sha512("def");
        assert!(five.has_digest());
    }

    #[test]
    fn temp_path_keeps_full_name() {
        let p = Path::new("/tmp/foo.jar");
        assert_eq!(temp_path(p), Path::new("/tmp/foo.jar.part"));
    }

    #[test]
    fn resume_offset_only_trusts_short_parts() {
        let dir = temp_dir();
        let tmp = dir.join("x.bin.part");

        // 没有 .part → 从零开始
        assert_eq!(resume_offset(&tmp, 100), 0);

        // 空 .part 也等于没有
        File::create(&tmp).unwrap();
        assert_eq!(resume_offset(&tmp, 100), 0);

        // 下了一半 → 接着下
        fs::write(&tmp, vec![7u8; 40]).unwrap();
        assert_eq!(resume_offset(&tmp, 100), 40);

        // 已经够长甚至更长：不可信，重下
        fs::write(&tmp, vec![7u8; 100]).unwrap();
        assert_eq!(resume_offset(&tmp, 100), 0);
        assert_eq!(resume_offset(&tmp, 0), 100);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_stale_parts_walks_and_filters() {
        let dir = temp_dir();
        let nested = dir.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        fs::write(dir.join("one.jar.part"), b"x").unwrap();
        fs::write(nested.join("two.jar.part"), b"x").unwrap();
        // 正常文件不能误删
        fs::write(nested.join("keep.jar"), b"x").unwrap();

        // 7 天内不清
        assert_eq!(cleanup_stale_parts(&dir, Duration::from_secs(7 * 86400)), 0);
        assert!(dir.join("one.jar.part").exists());

        // 阈值给 0 就全清掉
        assert_eq!(cleanup_stale_parts(&dir, Duration::from_secs(0)), 2);
        assert!(!dir.join("one.jar.part").exists());
        assert!(!nested.join("two.jar.part").exists());
        assert!(nested.join("keep.jar").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
