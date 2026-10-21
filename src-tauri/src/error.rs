use serde::{ser::SerializeStruct, Serialize, Serializer};

/// 统一错误类型。
/// 约定：所有面向用户的文案都写在 Display 里，不得把 Rust 内部 panic 信息透出。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("网络连接失败，请检查网络或代理设置")]
    Network,

    #[error("下载超时，可能是网络较慢或镜像源不可用")]
    Timeout,

    #[error("找不到指定的文件或目录：{0}")]
    NotFound(String),

    #[error("读取文件失败：{0}")]
    Io(String),

    #[error("数据格式不正确：{0}")]
    Parse(String),

    #[error("尚未配置 Java 运行环境，请先在设置中选择 Java 路径")]
    JavaMissing,

    #[error("指定的 Java 无法使用：{0}")]
    JavaInvalid(String),

    #[error("游戏版本不存在或已损坏：{0}")]
    VersionInvalid(String),

    #[error("启动游戏失败：{0}")]
    LaunchFailed(String),

    #[error("{0}")]
    Message(String),
}

impl AppError {
    /// 稳定的错误码，前端可据此做分支处理
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Network => "NETWORK",
            AppError::Timeout => "TIMEOUT",
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::Io(_) => "IO",
            AppError::Parse(_) => "PARSE",
            AppError::JavaMissing => "JAVA_MISSING",
            AppError::JavaInvalid(_) => "JAVA_INVALID",
            AppError::VersionInvalid(_) => "VERSION_INVALID",
            AppError::LaunchFailed(_) => "LAUNCH_FAILED",
            AppError::Message(_) => "MESSAGE",
        }
    }
}

/// 序列化为 { code, message }，前端拿到的永远是可读文案
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => AppError::NotFound(e.to_string()),
            _ => AppError::Io(e.to_string()),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Parse(e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            AppError::Timeout
        } else {
            AppError::Network
        }
    }
}
