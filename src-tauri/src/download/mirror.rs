//! 官方源与 BMCLAPI 镜像的切换策略。
//! 这里只做纯字符串映射与决策，不做 IO，方便单测。

pub const OFFICIAL_HOSTS: [&str; 4] = [
    "launchermeta.mojang.com",
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "launcher.mojang.com",
];

pub const MIRROR_BASE: &str = "https://bmclapi2.bangbang93.com";

/// 把官方 URL 映射为 BMCLAPI 等价 URL。
/// BMCLAPI 支持同路径替换，非官方域名原样返回。
pub fn to_mirror(url: &str) -> String {
    for host in OFFICIAL_HOSTS.iter() {
        let prefix = format!("https://{host}");
        if let Some(rest) = url.strip_prefix(&prefix) {
            return format!("{MIRROR_BASE}{rest}");
        }
        let prefix_http = format!("http://{host}");
        if let Some(rest) = url.strip_prefix(&prefix_http) {
            return format!("{MIRROR_BASE}{rest}");
        }
    }
    url.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceChoice {
    /// 优先官方，超时后切镜像
    OfficialFirst,
    /// 直接用镜像
    MirrorOnly,
}

/// 一次下载尝试的方案：先官方失败再镜像，或直接镜像
pub fn plan_urls(url: &str, choice: SourceChoice) -> Vec<String> {
    match choice {
        SourceChoice::MirrorOnly => vec![to_mirror(url)],
        SourceChoice::OfficialFirst => {
            let m = to_mirror(url);
            if m == url {
                vec![url.to_string()]
            } else {
                vec![url.to_string(), m]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_manifest() {
        let u = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
        assert_eq!(
            to_mirror(u),
            "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json"
        );
    }

    #[test]
    fn rewrite_piston_meta() {
        let u = "https://piston-meta.mojang.com/v1/packages/abc/26.3.json";
        assert_eq!(to_mirror(u), "https://bmclapi2.bangbang93.com/v1/packages/abc/26.3.json");
    }

    #[test]
    fn leave_unknown_host() {
        let u = "https://api.modrinth.com/v2/search";
        assert_eq!(to_mirror(u), u);
    }

    #[test]
    fn official_first_has_two_candidates() {
        let u = "https://piston-data.mojang.com/v1/objects/x/client.jar";
        assert_eq!(plan_urls(u, SourceChoice::OfficialFirst).len(), 2);
        assert_eq!(plan_urls(u, SourceChoice::MirrorOnly).len(), 1);
    }
}
