//! 带镜像回退的 HTTP 拉取。
//! 对上层只暴露 fetch_text / fetch_json，方便以后换异步实现或加进度回调。

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::download::mirror::{plan_urls, SourceChoice};
use crate::error::{AppError, AppResult};

/// 一次成功拉取的结果：内容 + 真实生效的 URL（前端可据此提示“当前走镜像”）
pub struct Fetched {
    pub body: String,
    pub url: String,
}

/// 按候选顺序逐个尝试：官方超时/非 2xx 时自动落到镜像。
pub fn fetch_text(
    client: &reqwest::blocking::Client,
    url: &str,
    choice: SourceChoice,
    timeout: Duration,
) -> AppResult<Fetched> {
    let mut last: Option<AppError> = None;
    for candidate in plan_urls(url, choice) {
        match try_once(client, &candidate, timeout) {
            Ok(body) => {
                return Ok(Fetched {
                    body,
                    url: candidate,
                })
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or(AppError::Network))
}

pub fn fetch_json<T: DeserializeOwned>(
    client: &reqwest::blocking::Client,
    url: &str,
    choice: SourceChoice,
    timeout: Duration,
) -> AppResult<(T, String)> {
    let got = fetch_text(client, url, choice, timeout)?;
    let parsed: T = serde_json::from_str(&got.body)?;
    Ok((parsed, got.url))
}

fn try_once(client: &reqwest::blocking::Client, url: &str, timeout: Duration) -> AppResult<String> {
    let resp = client.get(url).timeout(timeout).send()?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::Message(format!(
            "服务器返回 {}，该下载源可能暂不可用",
            status.as_u16()
        )));
    }
    Ok(resp.text()?)
}
