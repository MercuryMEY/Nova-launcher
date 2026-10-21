//! 崩溃日志分析：纯本地规则，不联网、不上传任何内容。
//!
//! 做法很朴素：把日志尾部逐行扫一遍，看有没有已知的「报错特征串」，
//! 命中就归到某条规则下，攒出证据行；最后一按严重度排序给结论。
//! 这样虽然抓不到「没见过的」崩溃，但能解释掉玩家八成以上的实际问题，
//! 而且不会把一个正常启动的日志误报成崩溃。
use serde::Serialize;

/// 单条证据最多保留的字符数：日志里偶尔有一整行 JSON / NBT 转储
const EVIDENCE_CLIP: usize = 240;
/// 每条规则最多留几行证据，多了界面会淹掉
const MAX_EVIDENCE: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// 基本可以确定就是这个原因
    Fatal,
    /// 可疑，需要人自己确认
    Warn,
    /// 只是提示，不一定是坏事
    Info,
}

#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    /// 行号，从 1 开始，相对传给 analyze 的那段
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub evidence: Vec<Evidence>,
    pub advice: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    /// 一句话结论
    pub headline: String,
    pub severity: Severity,
    /// 实际扫了多少行
    pub scanned: usize,
    /// 日志太大，只看了尾部
    pub truncated: bool,
    pub findings: Vec<Finding>,
    /// 退出码的解读，没传退出码就是 None
    pub exit_hint: Option<String>,
}

/// 一条规则：命中 needles 里任意一个字符串（忽略大小写）就算中
struct Rule {
    id: &'static str,
    severity: Severity,
    needles: &'static [&'static str],
    title: &'static str,
    detail: &'static str,
    advice: &'static [&'static str],
}

/// 规则表。顺序有意义：同一严重度下，靠前的会排在前面，
/// 所以把最常见的、结论最硬的放上面
const RULES: &[Rule] = &[
    Rule {
        id: "java_version",
        severity: Severity::Fatal,
        needles: &[
            "unsupportedclassversionerror",
            "only recognizes class file versions",
            "has been compiled by a more recent version",
        ],
        title: "Java 版本不对",
        detail: "游戏要求的 Java 大版本和你当前用的不一致，多半是版本太旧的 Java 去跑新版本的 MC",
        advice: &[
            "去设置 → Java 管理，点「自动选择」让它按版本要求挑运行时",
            "1.17 起需要 Java 17 以上，1.20.5 起需要 Java 21",
        ],
    },
    Rule {
        id: "out_of_memory",
        severity: Severity::Fatal,
        needles: &[
            "outofmemoryerror",
            "java heap space",
            "gc overhead limit exceeded",
            "cannot allocate memory",
        ],
        title: "内存不够",
        detail: "JVM 堆在运行中被撑满了，常见于大整合包或分配的内存太小",
        advice: &[
            "去设置 → 启动设置，把最大内存调大（整合包一般 4096 MB 起）",
            "别超过物理内存的一半，否则系统自己会开始换页，反而更卡",
            "同时确认系统层面没有别的东西在吃内存",
        ],
    },
    Rule {
        id: "gpu_driver",
        severity: Severity::Fatal,
        needles: &[
            "failed to create display",
            "glfw error",
            "pixel format not accelerated",
            "org.lwjgl.opengl.openglexception",
            "wglmakecurrent",
            "no opengl context",
        ],
        title: "显卡 / OpenGL 初始化失败",
        detail: "游戏没能拿到可用的 OpenGL 上下文，通常是驱动太旧、或者是远程桌面 / 虚拟机上跑",
        advice: &[
            "更新显卡驱动，尤其是 Intel 核显和 AMD 的老驱动",
            "如果是远程桌面或虚拟机，先在本机桌面会话里启动一次",
            "双显卡笔记本确认游戏跑在独显上（Windows 图形设置里指定）",
        ],
    },
    Rule {
        id: "native_crash",
        severity: Severity::Fatal,
        needles: &[
            "sigsegv",
            "exception_access_violation",
            "internal error (0xc0000005)",
            "hs_err_pid",
            "fatal error in native code",
        ],
        title: "JVM 之外的 native 层崩了",
        detail: "段错误 / 访问违例，Java 层接不住。几乎都是显卡驱动、输入法注入或某个 native 库的问题",
        advice: &[
            "先用原版（不装 Mod、不装加载器）启动一次，确认是不是环境问题",
            "换一个 Java 运行时试试，不同发行版带的 native 库不一样",
            "看看日志同级目录有没有 hs_err_pid*.log，里面会写明崩在哪个模块",
        ],
    },
    Rule {
        id: "missing_dependency",
        severity: Severity::Fatal,
        needles: &[
            "noclassdeffounderror",
            "classnotfoundexception",
            "nosuchmethoderror",
            "missing or unsupported mandatory dependencies",
            "modrequires",
        ],
        title: "缺少前置 Mod 或版本对不上",
        detail: "某个 Mod 依赖的类找不到：要么前置没装，要么前置的版本和它不匹配",
        advice: &[
            "回到 Mod 页，看日志里点名的那个 Mod 的「必需前置」有没有装齐",
            "前置版本要跟 Mod 自己声明的版本区间对得上，不是越新越好",
            "把最近新加的 Mod 先禁用掉一半，二分法最快定位",
        ],
    },
    Rule {
        id: "loader_failed",
        severity: Severity::Fatal,
        needles: &[
            "loadingfailedexception",
            "mod loading has failed",
            "incompatible mod set",
            "failed to load mod",
        ],
        title: "加载器没能把 Mod 集装起来",
        detail: "Forge / NeoForge / Fabric 在加载阶段就退出了，通常是 Mod 之间的依赖关系互相冲突",
        advice: &[
            "Forge 会单独生成一份崩溃报告，比游戏日志更清楚，先看那份",
            "一次只加一个 Mod 地试，或者按「最近改动」倒序禁用",
            "光影和优化 Mod（Sodium / OptiFine）之间最容易打架",
        ],
    },
    Rule {
        id: "duplicate_mods",
        severity: Severity::Warn,
        needles: &["duplicate mods", "duplicatemod", "already registered"],
        title: "同一个 Mod 装了两份",
        detail: "mods 目录里有重复的 Mod（常见于更新时没删旧文件，或者 jar 和 jar.disabled 同名）",
        advice: &[
            "去 Mod 页按名字排序，把重复的那份删掉",
            "注意别把同一个 Mod 的不同版本都留着",
        ],
    },
    Rule {
        id: "mod_file_broken",
        severity: Severity::Warn,
        needles: &[
            "invalid mod file",
            "zip end header not found",
            "invalidlocheader",
            "corrupted zip",
            "unexpected end of zlib",
        ],
        title: "有 Mod 文件是坏的",
        detail: "jar 下载不完整或解压出错，读取时 zip 结构就不对了",
        advice: &[
            "把日志里点名的那几个 jar 删掉重新装一次",
            "磁盘满了也会导致文件写不完整，顺手看一眼剩余空间",
        ],
    },
    Rule {
        id: "auth_failed",
        severity: Severity::Warn,
        needles: &[
            "invalid session",
            "authenticationexception",
            "failed to login",
            "invalid credentials",
            "401 unauthorized",
        ],
        title: "登录态失效",
        detail: "进服务器或联机验证时被拒，客户端本身的账号还在，只是令牌过期了",
        advice: &[
            "回账户页重新登录一次，令牌会自动刷新",
            "启动器会在启动前刷新一次令牌，如果一直失败，检查系统时间是否正确",
        ],
    },
    Rule {
        id: "missing_file",
        severity: Severity::Warn,
        needles: &["filenotfoundexception", "nosuchfileexception", "no such file or directory"],
        title: "游戏文件缺失",
        detail: "运行时找不到某个必须的文件，可能是安装没走完、或者被杀毒软件删掉了",
        advice: &[
            "去下载页对这个版本点「修复安装」，会把缺的文件补上",
            "把 .minecraft 目录加进杀毒软件的白名单",
        ],
    },
    Rule {
        id: "version_mismatch",
        severity: Severity::Warn,
        needles: &[
            "requires minecraft version",
            "incompatible minecraft version",
            "wrong minecraft version",
            "not compatible with",
        ],
        title: "Mod 和游戏版本不匹配",
        detail: "某个 Mod 只支持别的 MC 版本，装到了一个不兼容的版本目录里",
        advice: &[
            "确认 Mod 页当前选的目标版本就是这个版本",
            "装 Mod 时筛选条件里把游戏版本限定住，别让不兼容的混进来",
        ],
    },
    Rule {
        id: "mixin_failed",
        severity: Severity::Warn,
        needles: &["mixin apply failed", "mixin transformation", "org.spongepowered.asm.mixin"],
        title: "Mixin 注入失败",
        detail: "某个 Mod 想改写的游戏方法没找到，通常是它支持的 MC 版本和当前版本差了一点",
        advice: &[
            "查这个 Mod 在 Modrinth 上标注的 MC 版本区间",
            "多个优化 Mod 同时改同一处代码时也会这样，只留一个",
        ],
    },
    Rule {
        id: "crash_report",
        severity: Severity::Info,
        needles: &["---- minecraft crash report ----"],
        title: "这是游戏自己生成的崩溃报告",
        detail: "文件里带有完整的崩溃报告，比日志更有价值：里面有异常栈和 Mod 列表快照",
        advice: &[
            "往下翻到「Description」和「Caused by」两段，通常一眼就能看出原因",
        ],
    },
];

/// 退出码的通俗解读。Windows 上常见的是负的 0xC0000000 系列
fn exit_hint(code: i32) -> String {
    match code {
        0 => "退出码 0：进程是正常退出的，日志里的报错可能只是可恢复的警告".to_string(),
        1 => "退出码 1：普通异常退出，原因要看上面命中的报错".to_string(),
        134 => "退出码 134（SIGABRT）：进程被自己中止，多半是 native 层崩了".to_string(),
        137 => "退出码 137（SIGKILL）：被系统直接杀掉，最常见的原因是内存不足".to_string(),
        139 => "退出码 139（SIGSEGV）：段错误，通常是显卡驱动或 native 库的问题".to_string(),
        -1073741819 => {
            "退出码 0xC0000005（访问违例）：显卡驱动或某个 native 库崩溃，先更新驱动".to_string()
        }
        -1073741571 => {
            "退出码 0xC00000FD（栈溢出）：Mod 递归调用，或者世界生成时嵌套太深".to_string()
        }
        -1073740791 => {
            "退出码 0xC0000409（栈缓冲溢出）：native 层检测到越界，通常是驱动".to_string()
        }
        other => format!("退出码 {other}：不是常见值，只能结合上面的报错看"),
    }
}

fn clip(text: &str) -> String {
    let t = text.trim();
    if t.chars().count() <= EVIDENCE_CLIP {
        return t.to_string();
    }
    let head: String = t.chars().take(EVIDENCE_CLIP).collect();
    format!("{head}…")
}

/// 主入口：`lines` 是日志尾部（新的在后），`exit_code` 是进程退出码
pub fn analyze(lines: &[String], truncated: bool, exit_code: Option<i32>) -> Analysis {
    let mut hits: Vec<Vec<Evidence>> = RULES.iter().map(|_| Vec::new()).collect();
    let mut scanned = 0usize;

    for (i, raw) in lines.iter().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        scanned += 1;
        // 每行只转一次小写：一万行 × 十几条规则，重复转换会白白多跑几十万次
        let lower = raw.to_ascii_lowercase();
        for (ri, rule) in RULES.iter().enumerate() {
            if hits[ri].len() >= MAX_EVIDENCE {
                continue;
            }
            if rule.needles.iter().any(|n| lower.contains(n)) {
                hits[ri].push(Evidence {
                    line: i + 1,
                    text: clip(raw),
                });
            }
        }
    }

    let mut findings: Vec<Finding> = Vec::new();
    for (ri, rule) in RULES.iter().enumerate() {
        if hits[ri].is_empty() {
            continue;
        }
        findings.push(Finding {
            id: rule.id.to_string(),
            severity: rule.severity,
            title: rule.title.to_string(),
            detail: rule.detail.to_string(),
            evidence: std::mem::take(&mut hits[ri]),
            advice: rule.advice.iter().map(|s| s.to_string()).collect(),
        });
    }
    // 稳定排序：同级保持规则表顺序，界面上不会每次都不一样
    findings.sort_by_key(|f| match f.severity {
        Severity::Fatal => 0,
        Severity::Warn => 1,
        Severity::Info => 2,
    });

    let fatal = findings.iter().find(|f| f.severity == Severity::Fatal);
    let warn_count = findings.iter().filter(|f| f.severity == Severity::Warn).count();
    let (headline, severity) = if let Some(f) = fatal {
        (format!("最可能的原因：{}", f.title), Severity::Fatal)
    } else if warn_count > 0 {
        (
            format!("没有致命错误，但有 {warn_count} 条可疑线索"),
            Severity::Warn,
        )
    } else if scanned == 0 {
        ("这段日志是空的，没法分析".to_string(), Severity::Info)
    } else {
        (
            "没看到已知的崩溃特征，可能是卡死、被强杀，或者崩溃发生在日志之外".to_string(),
            Severity::Info,
        )
    };

    Analysis {
        headline,
        severity,
        scanned,
        truncated,
        findings,
        exit_hint: exit_code.map(exit_hint),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn ids(a: &Analysis) -> Vec<&str> {
        a.findings.iter().map(|f| f.id.as_str()).collect()
    }

    #[test]
    fn detects_out_of_memory() {
        let log = lines(&[
            "java.lang.OutOfMemoryError: Java heap space",
            "\tat net.minecraft.Client.run(Client.java:1)",
        ]);
        let a = analyze(&log, false, Some(1));
        assert_eq!(ids(&a), vec!["out_of_memory"]);
        assert_eq!(a.severity, Severity::Fatal);
        assert_eq!(a.findings[0].evidence[0].line, 1);
        assert!(a.headline.contains("内存"));
    }

    #[test]
    fn detects_java_version_problem() {
        let log = lines(&[
            "Error: LinkageError occurred while loading main class",
            "java.lang.UnsupportedClassVersionError: net/fabricmc/loader/impl/launch/server/FabricServerLauncher has been compiled by a more recent version of the Java Runtime (class file version 65.0)",
        ]);
        let a = analyze(&log, false, None);
        assert_eq!(ids(&a), vec!["java_version"]);
    }

    #[test]
    fn detects_gpu_and_native() {
        let log = lines(&[
            "GLFW error 65542: WGL: The driver does not appear to support OpenGL",
            "Failed to create display",
            "# A fatal error has been detected by the Java Runtime Environment:",
            "SIGSEGV (0xb) at pc=0x00007ff",
        ]);
        let a = analyze(&log, false, Some(139));
        assert!(ids(&a).contains(&"gpu_driver"));
        assert!(ids(&a).contains(&"native_crash"));
        assert!(a.exit_hint.unwrap().contains("SIGSEGV"));
    }

    #[test]
    fn detects_missing_dependency() {
        let log = lines(&[
            "java.lang.NoClassDefFoundError: net/fabricmc/api/EnvType",
            "\tat com.example.Mod.<init>(Mod.java:12)",
        ]);
        let a = analyze(&log, false, None);
        assert_eq!(ids(&a), vec!["missing_dependency"]);
    }

    #[test]
    fn clean_log_has_no_findings() {
        let log = lines(&[
            "[main/INFO]: Loading Minecraft 1.20.1 with Fabric Loader 0.15.11",
            "[main/INFO]: Setting user: Steve",
            "[main/INFO]: Backend library: LWJGL version 3.3.2",
        ]);
        let a = analyze(&log, false, Some(0));
        assert!(a.findings.is_empty());
        assert_eq!(a.severity, Severity::Info);
        assert_eq!(a.scanned, 3);
        assert!(a.headline.contains("没看到"));
    }

    #[test]
    fn fatal_sorts_before_warn() {
        let log = lines(&[
            "java.util.zip.ZipException: zip END header not found",
            "java.lang.OutOfMemoryError: GC overhead limit exceeded",
        ]);
        let a = analyze(&log, false, None);
        assert_eq!(ids(&a), vec!["out_of_memory", "mod_file_broken"]);
    }

    #[test]
    fn evidence_is_capped_and_clipped() {
        let long = format!("java.lang.OutOfMemoryError: {}", "x".repeat(600));
        let mut log = Vec::new();
        for _ in 0..10 {
            log.push(long.clone());
        }
        let a = analyze(&log, true, None);
        let f = &a.findings[0];
        assert_eq!(f.evidence.len(), MAX_EVIDENCE);
        assert!(f.evidence[0].text.chars().count() <= EVIDENCE_CLIP + 1);
        assert!(a.truncated);
    }

    #[test]
    fn exit_code_hints() {
        for (code, expect) in [
            (0, "正常退出"),
            (137, "内存不足"),
            (-1073741819, "访问违例"),
            (42, "不是常见值"),
        ] {
            assert!(exit_hint(code).contains(expect), "退出码 {code} 的解读不对");
        }
    }

    #[test]
    fn matching_is_case_insensitive() {
        let log = lines(&["JAVA.LANG.OUTOFMEMORYERROR: Java HEAP SPACE"]);
        let a = analyze(&log, false, None);
        assert_eq!(ids(&a), vec!["out_of_memory"]);
    }
}