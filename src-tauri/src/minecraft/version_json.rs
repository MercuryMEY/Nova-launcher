//! 版本 JSON 的结构映射。
//! 字段名严格按官方 schema（camelCase），所有块都允许缺失：
//! 1.6 以前没有 arguments / javaVersion，1.13 以前没有 assetIndex 的 url 兜底。

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default)]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub downloads: Option<Downloads>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    /// 1.6 之前的老格式：一整行参数
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub release_time: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub logging: Option<Logging>,
}

impl VersionJson {
    /// 缺失时按官方默认值兜底
    pub fn main_class_or_default(&self) -> String {
        self.main_class
            .clone()
            .unwrap_or_else(|| "net.minecraft.client.main.Main".to_string())
    }

    /// 版本要求的 Java 大版本，读不到就是 None（不能靠版本号猜）
    pub fn java_major(&self) -> Option<u32> {
        self.java_version.as_ref().map(|j| j.major_version)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub total_size: Option<u64>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Downloads {
    #[serde(default)]
    pub client: Option<Artifact>,
    #[serde(default)]
    pub server: Option<Artifact>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    /// 只有 logging 里的 file 会写这个字段（例如 client-1.12.xml），
    /// 其余位置缺失，所以是 Option
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    #[serde(default)]
    pub major_version: u32,
    #[serde(default)]
    pub component: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibDownloads>,
    /// Fabric / Quilt 的 profile 把仓库地址平铺在库对象上：
    /// `{"name":"org.ow2.asm:asm:9.6","url":"https://maven.fabricmc.net/","sha1":"...","size":123}`
    /// 没有 `downloads.artifact`，所以下载地址得靠这里的仓库根自己拼
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    /// 平台 -> classifier 名，值里的 ${arch} 需要运行时替换
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub extract: Option<Extract>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: HashMap<String, Artifact>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// 一条规则：action 为 allow / disallow，条件为空表示无条件命中
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: HashMap<String, bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    /// 官方历史上两种写法：正则字符串，或 `{ min, max }` 版本区间
    ///
    /// JSON 里的键名是 `versionRange`（不是 `version`）。之前少这行 rename，
    /// 版本条件整个被跳过，旧 Win10 也会拿到 -XX:+UseZGC
    #[serde(default, rename = "versionRange")]
    pub version: Option<VersionRange>,
}

/// versionRange 的两种形态。写成 untagged 是因为 26.3 的实际数据里两种都出现了：
/// 旧版本用 "^10\." 这种正则，新版本用 { "min": "10.0.17134" } 这种区间。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum VersionRange {
    Regex(String),
    Range {
        #[serde(default)]
        min: Option<String>,
        #[serde(default)]
        max: Option<String>,
    },
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgValue>,
    #[serde(default)]
    pub jvm: Vec<ArgValue>,
    /// 官方 JSON 里的 default-user-jvm，带连字符，必须单独 rename
    #[serde(default, rename = "default-user-jvm")]
    pub default_user_jvm: Vec<ArgValue>,
}

/// 参数项有两种形态：纯字符串，或 { rules, value } 条件项
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    Plain(String),
    Conditional {
        #[serde(default)]
        rules: Vec<Rule>,
        value: ArgValueValue,
    },
}

/// 条件项的 value 也可能是数组
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgValueValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logging {
    #[serde(default)]
    pub client: Option<ClientLog>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientLog {
    #[serde(default)]
    pub argument: Option<String>,
    #[serde(default)]
    pub file: Option<Artifact>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}
