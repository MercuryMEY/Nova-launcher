# Nova Launcher

基于 Tauri 2 + Vue 3 + Rust 的跨平台 Minecraft 启动器，功能对标 PCL2（非移植，完全重写）。

> 原名 MCLauncher，自 b1b2-fix1 起更名为 Nova Launcher。

## 当前进度

- [x] B1 项目骨架（Tauri 2 / Vue 3 / Pinia / Vite）
- [x] B2 前端外壳（标题栏、导航、主题与强调色、状态栏通知）
- [x] B2.1 代码审查后的可靠性修复（见下节）
- [x] B3 版本清单拉取 + rules 求值 + 启动参数组装
- [x] B4 离线账户（UUID 双模式 + 凭据存储抽象）
- [x] B5 Java 探测、运行时下载、natives 解压、真实启动与日志
- [x] B6 下载引擎与安装流程
- [x] B6.5 微软正版登录（设备码流程）
- [x] B7 加载器安装（Fabric / Forge / NeoForge / Quilt）
- [x] B8 Mod 管理（Modrinth 搜索 + 本地扫描 + 装 / 启停 / 删）
- [x] B9 整合包导入（.mrpack → instances/<id>/，含 overrides 与加载器）
- [x] B10 崩溃日志分析（纯本地规则，12 类常见崩溃 + 退出码解读）
- [x] B11 安全与完整性修复（下载校验、解压限额、外链白名单、实例 Mod、启动前检查）
- [x] B12 地基修稳 + 功能接线（图标 RGBA、启动顺序与互斥、设置接线、安装三态、断点续传、整合包补文件）

## b1b2-fix1 修复清单

| 问题 | 处理 |
|---|---|
| CSP 被关闭 | 已配置白名单 CSP，放行 `ipc:` / `http://ipc.localhost`，禁止 `object-src` |
| 原子写会丢配置 | 改为“临时文件 → 旧文件转 .bak → 就位 → 删备份”，任一步失败自动回滚 |
| 配置无版本号/无校验 | 增加 `version` 字段与 `migrate()`，内存、窗口尺寸、主题、当前账户均做兜底校验 |
| 初始化失败静默退出 | `AppState::load()` 不再失败退出，错误记入 `init_error`，由界面以通知展示 |
| 设置保存失败只进控制台 | 接入状态栏通知，同文案去重防刷屏 |
| 设置分类不能切换 | 分类已可切换，各分类内容独立 |
| 内存输入无校验提示 | 前端给出区间警告，后端 `clamp(512, 65536)` |

## B3 交付内容

### 新增模块

| 模块 | 职责 |
|---|---|
| `download/mirror.rs` | 官方地址 → BMCLAPI 地址映射；纯函数，带单测 |
| `download/plan.rs` | 按候选列表逐个重试；官方超时或非 2xx 自动落镜像 |
| `minecraft/manifest.rs` | 版本清单结构 + 拉取 |
| `minecraft/version_json.rs` | 版本 JSON 全字段映射（含 `default-user-jvm`、natives、logging） |
| `minecraft/rules.rs` | `os.name` / `os.arch` / `os.version` 正则 / `features` 求值 |
| `minecraft/classpath.rs` | maven 坐标 → 相对路径、natives 分类器、client.jar 收尾 |
| `minecraft/arguments.rs` | 新版 arguments 与旧版 minecraftArguments、`${...}` 占位符替换 |
| `minecraft/isolate.rs` | 三种隔离模式与目录布局 |
| `commands/version.rs` | 三个命令，全部 async + `spawn_blocking` |

### 新增命令

| 命令 | 返回 |
|---|---|
| `list_remote_versions` | `{ latest_release, latest_snapshot, source, cached, versions[] }` |
| `list_installed_versions` | `[{ id, path, java_major, has_json, isolated }]` |
| `preview_version_args(id)` | `{ main_class, java_major, game_dir, libraries, natives, client_jar_exists, missing_files[], jvm_args[], game_args[] }` |

前端新增 `src/api/version.ts`、`src/stores/version.ts`，启动页的版本列表已接入真实清单，并提供正式版/快照/已装/全部四种筛选、加载骨架、错误重试、参数展开。

### 几个关键设计

1. **Java 版本不靠版本号猜**，只读 `javaVersion.majorVersion`。真实数据里 `26.3` 要 Java 25，按老经验选 17/21 会直接启动失败。
2. **rules 求值**遵循官方语义：无 rules 默认放行；有 rules 时默认拒绝，从上到下逐条应用命中的 action，最后一条命中者说了算。`os.version` 按 Java 正则匹配。
3. **macOS 的 `-XstartOnFirstThread`** 是带 `os.name = osx` 规则的条件参数，漏掉就是黑屏，所以 rules 必须先求值再拼参数。
4. **清单拉取失败会退回本地缓存**（`<data_dir>/version_manifest.json`），界面会显示“本地缓存”，不是假装在线。
5. **三个命令都是 async + spawn_blocking**：Tauri 的同步命令跑在 UI 线程，拉清单一旦超时就会把窗口卡死。

### 自测

```bash
cd src-tauri
cargo test
```

单测覆盖：镜像地址重写、官方优先候选数、rules 平台/版本正则/feature 判定、占位符替换（含未知占位符保留）、旧版参数格式、数组型 value 展开、maven 路径拼接、client.jar 必须在 classpath 末尾。全部是纯函数测试，不联网。

## B3 真实数据仿真校验

用真实 `26.3.json`（Mojang 官方清单，javaVersion=25、114 个库、56 个 classifier 形式的 natives）独立复刻 rules / 参数逻辑，在 Python 里跑双平台对比，结论与服务端 Rust 代码一致：

| 检查项 | Windows 10.0.19045 | macOS 14.2 arm64 | 期望 |
| --- | --- | --- | --- |
| JVM 参数条数 | 17 | 17 | 分支互换、总数守恒 |
| `-XX:+UseZGC` | ✅ | ✅ | 两者都 > 10.0.17134 |
| G1 分支 | ❌ | ❌ | 仅老 Windows 命中 |
| `-XstartOnFirstThread` | ❌ | ✅ | 仅 macOS |
| HeapDumpPath | ✅ | ❌ | 仅 Windows |
| `-Xss1M` | ❌ | ❌ | 仅 arch=x86 |
| 堆参数 `-Xms/-Xmx` 已剥离 | ✅ | ✅ | 必须让位给设置页内存 |
| `-cp` 后紧跟 classpath | ✅ | ✅ | 顺序正确 |
| `--width 854` | ✅ | ✅ | 自定义分辨率生效 |
| `--demo` | ❌ | ❌ | features 未开启 |
| 普通库 / native 库 | 53 / 21 | 56 / 24 | 分类器识别正常 |

补充验证：Windows `6.1.7601`（Win7）命中 G1 分支、`10.0.22631`（Win11）命中 ZGC，说明 `cmp_version` 的分段数值比较没有踩字符串比较的坑。边界 `10.0.17134` 同时满足 min/max（Mojang 数据本身如此），行为与官方启动器一致，不作改动。

## B4 交付内容

| 层 | 文件 | 内容 |
|---|---|---|
| 后端 | `account/offline.rs` | 用户名校验、`OfflinePlayer:<name>` 派生 UUID（手写 MD5 + version3/variant，等价 Java 的 `UUID.nameUUIDFromBytes`）、随机 UUID |
| 后端 | `account/secret.rs` | 凭据存储抽象：Windows 凭据管理器 / macOS 钥匙串 / 内存兜底，系统调用失败自动降级并置 `degraded` |
| 后端 | `commands/account.rs` | `list_accounts`、`add_offline_account`、`update_offline_account`、`remove_account`、`set_current_account`、`secret_store_status` |
| 后端 | `state.rs` | `UuidMode` 枚举、`Account.uuid_mode` 字段、配置版本 1→2 与账户清洗、`current_account()` |
| 前端 | `api/account.ts`、`stores/account.ts` | 六个命令的类型与状态封装，写操作统一 busy 与通知 |
| 前端 | `pages/AccountPage.vue` | 账户列表、详情编辑、UUID 双模式切换、复制 UUID、二次确认删除、空状态 |
| 前端 | `SideNav.vue`、`App.vue`、`LaunchPage.vue` | 新增「账户」入口、`provide("navigate")` 跳转、启动页账户条显示头像/名字/模式 |

### 关键设计

1. **UUID 两种模式**：`name-based` 与离线服务器算法完全一致（同名同 UUID，多人服才不会串档）；`random` 用 v4，改名不换身份。默认 `name-based`，可逐账户切换
2. **降级要诚实**：系统凭据调用失败不阻断操作，但 `secret_store_status` 会返回 `degraded=true`，账户页顶部直接提示“密码不会持久化”
3. **改名策略**：`name-based` 改名重算 UUID，`random` 改名保留原 UUID——两条逻辑都写了注释，避免以后被当成冗余“优化”掉
4. **配置迁移**：旧配置（version 1）加载时自动补 `uuid_mode=name-based`、清洗空名字、修正长度不为 32 的 UUID；已经合法的 UUID 不重算，老账户身份不变

### 自测

- `cargo test`：`offline_uuid_matches_java_algorithm`（Notch / Steve / Player 三个固定值）、version 位、大小写敏感、名字长度与字符校验、内存凭据 round-trip
- 手工：添加 `Notch`，UUID 应为 `b50ad385-829d-3141-a216-7e7d7539ba7f`；切到随机后再改名，UUID 不变
- 查看 `config.json`：应出现 `"version": 2` 与每个账户的 `"uuid_mode"` 字段

## B5 交付内容
| 层 | 文件 | 内容 |
|---|---|---|
| 后端 | `download/file.rs` | 单文件流式下载：`.part` 临时名 + SHA1 校验 + 已就位直接跳过 |
| 后端 | `java/probe.rs` | 探测 PATH / JAVA_HOME / 各平台安装目录 / 手动路径，解析 `java -version`（输出走 stderr），按目标大版本挑最合适的一份 |
| 后端 | `java/runtime.rs` | 读 Mojang 运行时清单 `all.json`，选组件、按平台落地到 `<mc>/runtime/...`，逐文件校验 SHA1 |
| 后端 | `minecraft/natives.rs` | 按版本 JSON 解压 native 包到 `versions/<id>/natives/java`，同名同大小跳过，缺包只记录不报错 |
| 后端 | `launch/command.rs` | `build_plan()`：唯一的参数装配处，预览与真实启动共用 |
| 后端 | `launch/proc.rs` | 拉起进程、stdout/stderr 双线程写日志并推事件、看门狗轮询退出、`stop()` 最多等 2 秒 |
| 后端 | `launch/logs.rs` | 日志列表 / 尾部读取（最多 512KB）/ 删除，文件名一律挡 `..` 与路径分隔符 |
| 后端 | `commands/java.rs`、`commands/launch.rs` | 9 个命令：`list_java`、`java_status`、`install_java_runtime`、`launch_game`、`stop_game`、`game_status`、`list_logs`、`read_log`、`delete_log` |
| 前端 | `api/launch.ts`、`stores/launch.ts` | 命令与事件（`game:log` / `game:exit` / `launch:stage` / `java:progress`）封装，实时输出只在内存留 2000 行 |
| 前端 | `pages/LogsPage.vue` | 实时输出 + 历史日志列表、回看、删除、自动滚动到底 |
| 前端 | `pages/LaunchPage.vue` | 真实启动、停止、Java 状态条、一键下载运行时、启动阶段提示 |
| 前端 | `pages/SettingsPage.vue`、`SideNav.vue`、`App.vue` | Java 分类重做（扫描结果列表 + 手动路径 + 重新扫描）、新增“日志”入口 |

### 关键设计
1. **参数只有一份**：`preview_version_args` 现在也走 `build_plan`，界面上看到的 JVM 参数就是真正执行的那条命令行
2. **natives 目录不能随便挑**：26.3 的 `-Djava.library.path` 写死 `${natives_directory}/java`，所以解压目标是 `native_extract_dir()` 而不是 `natives_dir()`
3. **探测缓存要带上参数**：`probe::detect` 的缓存以前只存列表，刚装好的运行时会读到旧结果；现在把当时用的 `extra` 路径一起缓存
4. **缺文件先拦**：没有 `client.jar` 时 `precheck()` 直接拒绝启动，避免 JVM 抛一堆 `ClassNotFoundException`
5. **停止有兜底**：`kill()` 之后轮询 `try_wait()` 最多 2 秒，超时就明确报“进程没退出”，不假装成功

### 自测
- `cargo test`：Java 版本解析与 vendor 识别、组件挑选、运行时路径布局、natives 后缀判定、日志名路径穿越拦截、单文件下载“已就位”判定
- 手工：装一个 26.3 → 启动页应显示“需要 Java 25” → 没有 Java 时点“一键下载” → 装完再启动应生成 `logs/<id>-<时间>.log` 并实时滚动
- 缺 `client.jar` 时点启动应被拦下，提示补全客户端文件，而不是抛 `ClassNotFoundException`

## B6 交付内容
| 层 | 文件 | 内容 |
|---|---|---|
| 后端 | `download/pool.rs` | 有限并发下载池：N 线程抢一个队列、先判已就位再下、失败只收前 10 条并提示可重试 |
| 后端 | `install/plan.rs` | 安装计划构建：client.jar / libraries / assets / log4j 配置四类任务，assets 按 hash 前两位分目录 |
| 后端 | `install/executor.rs` | 分阶段执行：客户端（单文件直下）→ 依赖库 → 日志配置 → 资源，每阶段回传进度 |
| 后端 | `commands/install.rs` | 4 个命令：`preview_install`、`start_install`、`cancel_install`、`install_busy` |
| 前端 | `api/install.ts`、`stores/install.ts` | 命令与事件（`install:stage` / `install:progress` / `install:done`）封装，进度条按阶段刷新 |
| 前端 | `pages/DownloadsPage.vue` | 真实下载页：版本筛选、选中即预览、进度条、取消、修复安装 |

### 关键设计
1. **预览与安装共用一份计划**：`preview_install` 与 `start_install` 都调 `install::plan::build`，唯一区别是预览不把 asset index 写入磁盘，界面上的数字不会和实际下载量对不上
2. **超时不能沿用设置的 4 秒**：`timeout_of()` 取 `mirror_timeout_secs * 5` 且下限 60 秒；client.jar 和部分资源都是几十 MB，4 秒必失败
3. **没上异步运行时**：项目里全是 reqwest blocking，为下载再引一套 tokio 不划算，4 个线程抢一个 `VecDeque` 已能跑满带宽
4. **同时只允许一个安装**：`INSTALLING` 原子标志拦并发，`BusyGuard` 在作用域结束时复位，失败过一次也不会卡住后续安装
5. **取消是协作式的**：`cancel_install` 只置标志，下载线程取下一个任务时自行退出；已下完的文件保留，再点一次会跳过
6. **版本 JSON 先落盘**：先写 `.part` 再改名，下载失败也能在版本列表里看到这一版并重试
7. **资源直连官方而非镜像**：BMCLAPI 的 assets 路由与官方不同形，宁可连官方也不拼一个可能 404 的地址

### 自测
- `cargo test`：下载池空任务直接返回、取消前置生效、阶段文件数统计、超时下限、计划体积求和
- 手工：下载页选 26.3 → 显示“共 N 个文件，约 X MB” → 点安装 → 进度条依次走客户端 / 依赖库 / 日志配置 / 资源 → 完成后按钮变“修复安装”
- 再点一次安装应几乎全部跳过；中途取消后再重试能接着下

## B6.5 交付内容（微软正版登录）
| 层 | 文件 | 内容 |
|---|---|---|
| 后端 | `account/microsoft.rs` | 设备码流程：申请票据 → 轮询 → 微软令牌 → Xbox Live（RPS）→ XSTS → Minecraft 令牌 → 官方档案；含刷新、内存会话缓存、错误码翻译 |
| 后端 | `account/mod.rs` | 三个子模块：`offline` / `microsoft` / `secret` |
| 后端 | `commands/microsoft.rs` | 4 个命令：`ms_login_start`、`ms_login_poll`、`ms_account_state`、`ms_login_cancel` |
| 后端 | `commands/account.rs` | `AccountView` 增加 `has_secret`；`view_of` 抽成公开函数供微软命令复用；删账户改走 `microsoft::forget`（连带清内存令牌） |
| 后端 | `launch/command.rs` | 拆成 `build_plan`（预览，占位身份）与 `build_plan_with_auth`（启动，真实令牌），入口新增 `AuthTokens` |
| 后端 | `commands/launch.rs` | 启动前对微软账户调 `ensure_session` 取票，离线账户直接走占位身份 |
| 前端 | `api/microsoft.ts`、`stores/account.ts` | 命令封装 + 设备码轮询状态机（定时器、slow_down 退避、取消） |
| 前端 | `pages/AccountPage.vue` | 微软登录入口、8 位码放大显示、复制 / 打开浏览器、登录状态与“重新登录” |
### 关键设计
1. **用设备码而非回调**：桌面端等回调要起本地 HTTP 服务、处理端口占用和防火墙提示；设备码只需在浏览器里输 8 位码，Windows / macOS / Linux 一套逻辑
2. **后端不替前端睡**：`ms_login_start` 只发票据，`ms_login_poll` 每次只问一次，节奏由前端定时器控制——界面才随时能取消，也不用占一个线程等十几分钟
3. **同一时间只允许一个登录**：`PENDING` 是全局 `Mutex<Option<DeviceCodeTicket>>`，两套票据互相覆盖会让 8 位码对不上
4. **refresh token 是唯一落盘的秘密**：只进 `secret` 抽象（Windows 凭据管理器 / macOS 钥匙串 / 其余平台内存降级）；Minecraft access token 只留内存缓存，`AccountView` 永不携带任何 token
5. **预览与启动共用装配点**：`build_plan` 只是“占位身份 + 调 `build_plan_with_auth`”，参数形态仍然只有一份，界面看到的和真正启动的一致
6. **重复登录复用原条目**：按 `kind == Microsoft && uuid == session.uuid` 找已有账户，否则同一个微软号会在列表里出现两次
7. **启动前只刷一次票**：`ensure_session` 先看内存缓存（提前 60 秒判过期），过期才用 refresh token 换新；不做后台定时刷新，避免无谓网络请求
8. **XSTS 错误码翻成人话**：`2148916233/35/36/37/38` 分别提示“没买过 Minecraft”“未成年需家长同意”“账号地区受限”等，用户不用去搜错误码
### 自测
- `cargo test`：token 响应解析（成功带 refresh / pending / slow_down / denied / expired / 未知错误保留描述）、XSTS 提示文案、官方 UUID 去横线小写（`069A79F444E94726A5BAE090B6523B00` → `069a79f444e94726a5bae090b6523b00`）
- 手工：账户页点“使用微软账户登录”→ 显示 8 位码 → 打开浏览器输入 → 回到应用等到“已登录”→ 列表出现带“正版”标记的账户 → 启动游戏能进正版服
- 重启应用后该账户显示“凭据已保存”，再启动不需要重新登录（首次启动会静默续期）

## B7 交付内容（加载器安装）
| 层 | 文件 | 内容 |
|---|---|---|
| 后端 | `minecraft/inherit.rs` | 版本 JSON 的 `inheritsFrom` 父子合并：主类/资源/Java 取子优先，库按 `group:artifact` 去重且子覆盖父但保位置，参数父前子后拼接；限深 4 层防环 |
| 后端 | `minecraft/version_json.rs` | `Library` 增加平铺的 `url`/`sha1`/`size`（Fabric / Quilt 的 profile 就这么写，没有 `downloads.artifact`） |
| 后端 | `minecraft/classpath.rs` | `entry()` 收 `&Library`，`artifact.url` 为空时用仓库根拼 maven 路径；sha1/size 同样兜底 |
| 后端 | `loader/meta.rs` | 四种加载器的版本列表（官方 + BMCLAPI 镜像）、Fabric/Quilt profile、Forge/NeoForge 安装器 URL、版本目录命名、安装器所需 Java 版本推导 |
| 后端 | `loader/install.rs` | 两条安装路径：profile 型（落 JSON + 下库 + 复制 client.jar）与 installer 型（选 Java、下安装器、跑子进程、按目录差集定位新版本） |
| 后端 | `commands/loader.rs` | 4 个命令：`loader_kinds`、`loader_versions`、`loader_install`、`loader_installed` |
| 后端 | `launch/command.rs` | `load_version` 改走继承合并（否则加载器版本只有半份信息）；新增 `load_version_raw` 供诊断 |
| 后端 | `commands/version.rs` | 已装列表的 Java 版本也走继承合并，加载器版本不再显示“未知 Java” |
| 前端 | `api/loader.ts`、`stores/loader.ts` | 命令与事件（`loader:stage` / `loader:progress` / `loader:log` / `loader:done`）封装，安装器日志只在内存留 200 行 |
| 前端 | `pages/DownloadsPage.vue` | 选中原版版本后出现「装加载器」区域：四种加载器切换、版本下拉、进度条、安装器实时输出、已装清单 |
### 关键设计
1. **先补继承合并，再谈装加载器**：`VersionJson` 早就解析了 `inheritsFrom`，但 `load_version` 只读单个文件。不合并的话加载器的 asm 版本覆盖不到父版本、assets 拿不到、mainClass 也是错的，装完只能启动到一半崩掉
2. **库去重键是 `group:artifact`，不含版本**：这样子的 `asm:9.6` 能顶掉父的 `asm:9.5`；覆盖后**保持父位置**，否则 LWJGL 这类对顺序敏感的库会被挤到后面，classpath 顺序一乱就出 `NoSuchMethodError`
3. **Forge / NeoForge 必须传 `--installClient <dir>`**：不传会装进用户默认的 `.minecraft`，启动器管不到那份文件；同时加 `-Djava.awt.headless=true`，防止某些平台弹 Swing 界面卡死
4. **装完的目录名不能硬编码**：安装器自己决定叫 `1.20.1-forge-47.2.0` 还是别的，所以用“装前没有、装后多出来”的目录差集，优先挑 `inheritsFrom` 指向本体那个；差集为空（重装）才按命名规则兜底
5. **stderr 单独开线程收**：安装器失败时异常栈都在 stderr，主线程要专心转发 stdout；stderr 上限 200 行、stdout 只留最后 5 行拼进错误信息，够定位又不刷屏
6. **加载器版本要有自己的 client jar**：官方启动器也是把原版 jar 复制进加载器版本目录，直接指向原版目录会让部分版本的资源查找与 `--versionType` 对不上
7. **Fabric / Quilt 先落盘再读回合并**：profile 拿到的是原始 JSON，先写 `<versions>/<id>/<id>.json` 再用 `inherit::load_with_parent` 读，这样“装了哪些库”与实际磁盘状态永远一致
8. **NeoForge 的版本号要映射**：`21.1.9` → MC `1.21.1`、`21.0.3` → `1.21`，从 maven-metadata.xml 里手写扫描 `<version>` 再按映射筛，为这点事引 XML 库不划算
### 自测
- `cargo test`：继承合并 7 项（主类取子、库去重保位、参数拼接、资源/Java 继承、去重键剥离版本与 classifier、`downloads` 保留、父无 downloads 时子地址仍在）、加载器元数据 9 项（Forge 前缀不吞邻居版本、NeoForge 版本映射、XML 扫描新的在前、四种目录命名、安装器 Java 版本推导、安装器 URL 形态、类型往返）、profile 型库任务过滤
- 手工：下载页装好 1.20.1 原版 → 「装加载器」选 Fabric → 选推荐版本 → 装完版本列表出现 `fabric-loader-…` → 启动页选它进游戏
- 手工：同页面切 Forge，会提示需要 Java 并跑安装器，日志区实时滚动；装完再点一次应能识别已装版本而不是重复报错
- 回归：原版启动、预览参数、日志页都不受影响（父版本没有 `inheritsFrom` 时直接返回原 JSON）

## B8 交付内容（Mod 管理）
| 层 | 文件 | 内容 |
|---|---|---|
| 后端 | `mods/modrinth.rs` | Modrinth API v2 客户端：搜索（关键词 + facets + 排序 + 分页）、某项目在指定 MC 版本 / 加载器下的文件列表；facets 内层 AND 外层 OR，排序走白名单，主文件按 `primary` 挑 |
| 后端 | `mods/local.rs` | 本地扫描：逐个进 jar 读 `fabric.mod.json` / `quilt.mod.json` / `META-INF/neoforge.mods.toml` / `META-INF/mods.toml`，自带一个够用的 TOML 取值器；`.jar.disabled` 视为已禁用 |
| 后端 | `mods/install.rs` | 下载安装、启停改名、删除；文件名先过白名单（拒空、拒 `/` `\` `..`、必须以 `.jar` 结尾） |
| 后端 | `commands/mods.rs` | 7 个命令：`mods_search`、`mods_versions`、`mods_install`、`mods_list`、`mods_dir_of`、`mods_toggle`、`mods_remove` |
| 前端 | `api/mods.ts`、`stores/mods.ts` | 命令封装 + 状态；`detectLoader` / `baseGameOf` 从版本目录名还原加载器与原版版本 |
| 前端 | `pages/ModsPage.vue` | 左侧目标版本 + 本地 Mod 列表（启停 / 删除二次确认 / mods 路径），右侧搜索结果与项目详情（文件名、大小、发布时间、必需前置数量） |

### 关键设计
1. **Mod 装进哪个目录由隔离模式决定**：路径走 `isolate::game_dir(root, id, mode)` 再拼 `mods`。默认 `non-fake` 模式下只要 `versions/<id>/mods` 存在，这个版本就自动进入隔离，不同版本的 Mod 不会串档 —— 换句话说“装第一个 Mod”顺便就把隔离开了
2. **搜索和下载都在 `spawn_blocking` 里**：两个都是网络活，放在 Tauri 的命令线程上会把窗口卡住；命令本身是 `async`，Rust 侧阻塞不占 runtime 线程
3. **文件名来自网络，直接拼路径就是路径穿越**：`safe_file_name` 只放行“普通文件名 + `.jar` 结尾”，`../evil.jar`、`a/b.jar`、`readme.txt` 全部拒掉；版本目录名同样过一遍校验
4. **启停靠改名不靠删文件**：禁用 = 加 `.disabled` 后缀，游戏与 Forge 都不会加载；重新启用就去掉。只认 `.jar.disabled`，`readme.disabled` 不算 Mod；重复点同一状态直接返回，不报错
5. **版本目录名反推加载器和原版版本**：`fabric-loader-0.16.5-1.21.4` 的 MC 版本在最后一段，`1.21.4-forge-54.1.0` 在第一段，靠这个决定搜索 facets 与目标目录，用户只需要选“哪个版本”
6. **前置只提示，不代装**：Modrinth 的 `required` 依赖只能反查到 project id，拿不到中文名；代装会牵出一串递归依赖，风险比收益大，所以在版本条目上写“必需前置 N 个”，玩家自己决定
7. **Modrinth 要求带 `User-Agent`**：不带会被限流甚至 403，所以所有请求统一加 `NovaLauncher/0.1.0 (Mod browsing)` 这个标识
8. **读不到元数据不算错**：jar 里描述文件缺失、写坏、字段名奇怪都当作“未知 Mod”，只显示文件名，不影响启停和删除

### 自测
- `cargo test`：Modrinth 8 项（facets 三态、排序白名单、`primary` 挑选、无 `primary` 回退、空 url 跳过、加载器解析宽容）、本地扫描 8 项（禁用后缀、TOML 双引号、注释与前缀干扰、单引号与裸值、Fabric 解析、无 id 拒绝、Quilt 嵌套、TOML 加载器归属）、文件操作 4 项（合法名、穿越拒绝、非 jar 拒绝、启停目标名）、命令层 1 项（版本名校验）
- 手工：Mod 页选 `fabric-loader-…` → 搜一个 Mod → 点开 → 装 → 左侧出现条目 → 禁用变灰 → 启用恢复 → 删除点两下生效
- 手工：同一个文件再装一次，应提示“已经在 mods 里了”而不是重新下载
- 手工：切到一个没装加载器的原版版本，加载器显示「不限」，搜索结果会混入 Forge 专属 Mod（这是预期的，提示语里已写明）


## B9 交付内容（整合包导入）
| 层 | 文件 | 内容 |
|---|---|---|
| 后端 | `modpack/mrpack.rs` | `.mrpack` 解析：从 zip 里只读 `modrinth.index.json`，校验格式版本、取 MC 版本与加载器版本、过滤纯服务端文件、`safe_rel_path` 防路径穿越、实例 id 洗白 |
| 后端 | `modpack/install.rs` | `plan()` 算实例 id（重名自动 `-2`）、`import()` 解 overrides / client-overrides + 逐文件下载 + 写 `instance.json`；`list` / `remove` / `set_launch_id` |
| 后端 | `commands/modpack.rs` | 5 个命令：`modpack_inspect`、`modpack_import`、`modpack_list`、`modpack_remove`、`modpack_format` |
| 后端 | `launch/command.rs`、`commands/launch.rs` | `build_plan_with_auth` 增加 `game_dir` 覆盖参数；`launch_game` 增加可选 `instance`，先过白名单再决定 gameDir |
| 前端 | `api/modpack.ts`、`stores/modpack.ts` | 命令封装 + 事件绑定（`modpack:stage` / `progress` / `log` / `done`），日志只留最后 400 行 |
| 前端 | `pages/ModpackPage.vue` | 系统文件框选包 → 预览卡（版本 / 加载器 / 文件数 / 体积 / 实例目录 / 重名与缺原版提示）→ 进度条 + 日志 → 实例列表（启动 / 二次确认删除） |
### 关键设计
1. **实例目录和版本目录是两层**：`instances/<id>/` 只放存档、Mod、配置（也就是 gameDir），版本 JSON、加载器、libraries 仍在 `versions/<launch_id>/`。启动时 `launch_game(id = launch_id, instance = <实例 id>)`，两个参数各管一头
2. **实例 id 来自包名，重名自动加后缀**：`plan()` 时就从 `名字` 洗白出目录名，已存在则试 `-2`、`-3`；预览使用同一个函数，所以预览看到的 id 就是真正会创建的 id
3. **`overrides` 先铺、`client-overrides` 后铺**：后者本来就是给客户端专用的覆盖层，顺序反了会被通用文件盖掉
4. **单个文件失败不整体回滚**：一个失效的 CDN 链接不该让几十个已下好的文件白做一遍，失败只记日志与计数，重导一遍会按 `already_ok` 跳过已有文件
5. **加载器失败不算导入失败**：文件已经在盘上了，这时只把 `launch_id` 留空并在日志里说清楚，用户装好原版或加载器后再回来启动
6. **原版没装就跳过加载器**：Forge / NeoForge 安装器要求原版在本地，所以预览卡上直接提示，并把复选框置灰
7. **所有 zip 内路径都过 `safe_rel_path`**：拒绝空、绝对路径、`:`（盘符 / NTFS 流）、`..` 段，反斜杠统一成正斜杠 —— 整合包是别人给的 zip，不能信里面的路径
8. **时间戳只存 Unix 秒**：后端不引 chrono，格式化交给前端 `new Date(sec * 1000)`
### 自测
- `cargo test`：`.mrpack` 8 项（最小索引、服务端文件过滤、缺 `minecraft` 报错、格式 v2 拒绝、三种加载器键、非 http 下载地址过滤、路径越界拒绝、id 洗白）、实例 3 项（目录布局、删除越界、缺失实例返回 None）
- 手工：选一个 `.mrpack` → 预览信息与包内 `modrinth.index.json` 一致 → 导入 → 进度条走完 → 左侧出现实例 → 点启动进游戏 → `instances/<id>/mods` 里有 Mod
- 手工：同名包再导一次，预览卡应提示“另存为 名字-2”，导入后两个实例并存
- 手工：取消勾选“顺便安装加载器”，导入完成后实例显示「加载器没装」，启动按钮为灰

## B10 交付内容（崩溃日志分析）
| 层 | 文件 | 内容 |
|---|---|---|
| 后端 | `crash/mod.rs` | 12 条规则表（Java 版本 / 内存不足 / 显卡驱动 / native 段错 / 缺依赖 / 加载器失败 / Mod 重复 / Mod 文件损坏 / 认证失败 / 缺文件 / 版本不符 / Mixin 崩）+ 一条 `crash_report` 提示；`exit_hint` 解读退出码；`analyze()` 主入口 + 9 个单测 |
| 后端 | `commands/crash.rs` | `crash_analyze(name?, code?, lines?)`：不传文件名就取最新那份日志，尾部行数默认 4000、夹在 200~20000 之间 |
| 前端 | `api/crash.ts`、`stores/crash.ts` | 类型与命令封装；store 只保留 `analysis` / `busy` / `error` / `name` 和一次 `run()`，分析是纯读操作，不写盘 |
| 前端 | `pages/CrashPage.vue` | 进页面自动分析最新日志；结论卡（严重度、一句话结论、退出码解读、扫描行数、是否只看尾部）+ 逐条问题卡（标题、说明、带行号的证据原文、建议列表）+「看原始日志」跳转 |
### 关键设计
1. **纯本地、零联网**：整条链路没有一次 HTTP，日志内容不出机器。代价是规则质量只能靠人工攒，没见过的崩溃抓不到
2. **规则是一张静态表**：每条是 `Rule { id, severity, needles, title, detail, advice }`，加规则只往表里追加，分析逻辑不用动；`needles` 全是小写特征串，匹配前每行只做一次 `to_ascii_lowercase()`
3. **三档严重度 + 稳定排序**：结果按 `fatal > warn > info` 排，同级保持规则表顺序；`headline` 取第一条 fatal，没有 fatal 就退到 warn 的第一条
4. **证据要留，但不能淹**：每条规则最多留 3 行证据，单行截到 240 字符 —— 日志里偶尔有整行 JSON / NBT 转储，不截会把界面撑爆
5. **退出码单独解读**：覆盖 `0` / `1` / `134`（abort）/ `137`（被强杀）/ `139`（段错误）以及 Windows 的 `0xC0000005` / `0xC00000FD` / `0xC0000409`，其余一律说「不是常见值」，不硬猜
6. **不重写读日志**：直接复用 `launch::logs` 的 `list` 和 `read_tail`，文件名校验（挡 `..`、分隔符、隐藏文件）、尾部 512KB 上限、行数夹取全都只有一份实现
7. **前端只认 `id`**：卡片渲染全靠后端返回的 `title` / `detail` / `advice`，`id` 只当列表 key。以后加规则，前端一行都不用改
8. **不缓存分析结果**：日志会被下一次启动覆盖，缓存下来的「结论」只会误导人，所以每次进页面都重新扫
### 自测
- `cargo test crash`：9 项 —— 内存不足、Java 版本、显卡与 native 段错、缺依赖、干净日志不误报、fatal 排在 warn 之前、证据上限与单行截断、退出码文案、匹配大小写不敏感
- 手工：正常启动再退出，进崩溃分析页点「分析最新日志」，应当没有任何 fatal 命中
- 手工：把最大内存调到 256 MB 启动大整合包，触发 OOM 后再分析，应命中「内存不足」并给出加内存建议
- 手工：挑一份老日志并把退出码填 `-1073741819`，结论卡应显示访问冲突的中文解读

## B11 交付内容（安全与完整性修复）
| 层 | 文件 | 内容 |
|---|---|---|
| 后端 | `download/file.rs` | 整份重写：`FileTask` 增加 `sha512` 与链式 `with_sha512()`、`has_digest()`；`already_ok()` 判定顺序改为 sha512 → sha1 → 声明大小 → 直接拒绝；`download_file()` 在算摘要前先比对字节数 |
| 后端 | `fsutil.rs` | 新增 `sha512_file()`，与 `sha1_file()` 同样流式 64 KB 读，不把文件塞进内存 |
| 后端 | `modpack/install.rs` | 解压加三道上限：单条 512 MB / 总计 2 GB / 条目数 20000；抽出纯函数 `check_quota()` 便于单测；`.mrpack` 里带的 sha512 现在真的会传下去校验 |
| 后端 | `commands/mod.rs` | 新增 `is_safe_url()`：字符白名单 + 长度上限 + 必须有主机名；Windows 分支从 `cmd /C start` 换成 `rundll32.exe url.dll,FileProtocolHandler` |
| 后端 | `commands/mods.rs` | 新增 `safe_instance_id()` 挡路径穿越；五个 Mod 命令都加了 `instance` 参数，实例模式下目录走 `instances/<id>/mods` |
| 后端 | `launch/command.rs` | `precheck()` 增加依赖库检查与 natives 目录检查，复用已有的 `missing_files`，不额外扫盘 |
| 前端 | `api/mods.ts`、`stores/mods.ts` | 五个接口带 `instance`；store 增加 `instanceId` / `instanceGame` / `instanceLoader` / `setInstance` 与 `pathArg` 换算 |
| 前端 | `pages/ModsPage.vue` | 目标选择改成「版本 / 实例」两个 tab，各自一个列表；实例带出游戏版本与加载器一行小字 |
| 前端 | `stores/launch.ts`、`pages/LaunchPage.vue` | 新增 `lastError`；启动失败时页面出现错误条，缺库/缺 native 时额外给「去下载页修复」按钮 |

### 关键设计
1. **`FileTask::new` 的四参数签名不动**：sha512 走 `with_sha512()` 链式补。调用点有十几处，改签名等于全量返工，而链式调用只在需要时多写一行
2. **摘要优先级 sha512 → sha1 → 大小 → 拒绝**：只有 sha512 能覆盖整合包那种「sha1 字段为空」的来源；两者都没有、连大小也没有时不再乐观跳过 —— 宁可多下一次，也不留一个可能是残包的文件
3. **先比大小，再算摘要**：CDN 提前掐断连接时 `content-length` 会跟着变小，跟着它校验等于自欺欺人。清单里声明的字节数才是准的，所以先拿实际写入量对声明值
4. **失败只动 `.part`**：校验不过只删临时文件，已经存在的同名文件一个字节都不碰。用户拿别的启动器装好的文件不该被我这边误删
5. **解压限额双保险**：写之前按 zip 头里的 `entry.size()` 拦一次（快，防 zip bomb），写之后按 `io::copy` 实际复制的字节数再累加拦一次（防 header 撒谎）。`check_quota()` 单独抽出来是因为真造一个 2 GB 的 zip 跑测试不现实
6. **Windows 外链不走 shell**：`cmd /C start` 的问题不是白名单能兜住的 —— Rust 的 `Command::args()` 不加引号，URL 里的 `&` 会被 cmd 当成命令分隔符。换成 `rundll32.exe url.dll,FileProtocolHandler` 走 ShellExecute，压根不经 shell；字符白名单保留作纵深防御
7. **Mod 目录只有一个判断点**：所有路径都由 `mods_path(state, game, instance)` 算出来，`instance` 为空才落到 `versions/<id>/mods`。分散判断迟早会有一处漏掉，改的时候还容易漏改
8. **precheck 不新增扫描**：`build_plan` 本来就在收集 `missing_files`（上限 20 条），precheck 只是把结果用起来，取前 3 个文件名报给用户

### 自测
- `cargo test`：新增 sha512 校验通过/不通过、无摘要无大小时拒绝、大小不符先报错、解压配额三项（单条超限 / 总量超限 / 条目数超限）、`is_safe_url` 四组（正常放行 / 危险字符 / 超长 / 无主机名）、`safe_instance_id` 挡 `..`、precheck 四组（缺库 / 缺 client.jar / 缺 native / 正常放行）
- 手工：删掉版本目录里某个库文件，点启动，应直接被拦住并提示「依赖库缺失」，同时页面出现「去下载页修复」
- 手工：导入一个 `.mrpack`，看日志里是否出现 sha512 校验；再手动改坏一次 zip，确认会报配额或校验错误而不是把磁盘写满
- 手工：在 Mod 页切到「实例」tab，选一个整合包实例，装上/停用一个 Mod，确认落在 `instances/<id>/mods` 而不是版本目录

## B12 交付内容（地基修稳 + 功能接线）

### 图标与编译地基
| 项 | 处理 |
|---|---|
| 致命：图标不是 RGBA | `generate_context!()` 要求 PNG/ICO 为 RGBA，旧图标是 RGB（color type 2），宏展开阶段直接 panic。B9~B11 的“静态审通过”因此从未真正编译过。全部图标重新生成为透明背景 + 蓝紫渐变 + 白色几何 N，`tauri.conf.json` 补 `icons/64x64.png` |
| 测试编译错误 | `inherit.rs` 里 `m.asset_index.unwrap()` 造成部分移动（E0382），改为先取引用再比对 |
| 6 个失败用例 | Java 运行时测试写错断言、快照版本号被判成 Java 8、NeoForge 版本用字典序排序、`OsRule.versionRange` / `PackFile.fileSize` / `Index.formatVersion` 缺 serde 重命名，全部修掉 |

### 启动链路
| 项 | 处理 |
|---|---|
| precheck 早于 natives 解压 | 拆成 `precheck_core()`（client.jar + 缺库）与 `precheck_natives()`（native 数 > 0 时目录非空）。顺序改为 core → extract → natives → Java，老版本首次启动不再被自己拦住 |
| 并发启动窗口 | 新增 `launch/lock.rs`：`LaunchGuard` 用 `compare_exchange` 抢锁、`Drop` 释放，`AppState` 多一个 `launching: Arc<AtomicBool>`。不用 `Mutex<bool>` 是因为 `MutexGuard` 不是 `Send`，跨 `await` 持有会编译失败 |
| 设置没接线 | `window_width/height` 明确为游戏分辨率（拼 `--width/--height`）；新增 `fullscreen_mode` 三态（ignore / on / off），启动前只改该版本 `options.txt` 的 `fullscreen:` 行；`launcher_width/height/maximized` 在 `.setup()` 里应用，`save_settings` 落盘成功后才动窗口 |

### 安装可靠性
| 项 | 处理 |
|---|---|
| 安装状态只有布尔 | `Planned::missing_required()` 逐项比对磁盘，前端拿到 `status`（missing / partial / complete）+ `missing_count` + 最多 20 个 `missing_names`，按钮文案分三档 |
| 下载无断点续传 | `.part` 续写：206 追加、200 截断重下、416 删掉重来，最多两轮；`done` 初值等于已有字节；安装开始时顺手清 7 天前的 `.part` |
| 整合包失败后无法补 | `Instance` 增 `failed: Vec<String>` 与 `source: Option<String>`，新增 `modpack_retry(id)`：只重下失败条目，不重解压 overrides、不重装加载器 |

### 清理
- 删除确认无用的函数：`parse_vendor`、`load_version_raw`、`NativesReport::is_empty`、`launch::lock::is_busy`、`mods::modrinth::parse_loader`、`minecraft::manifest::fetch_version_json`
- 清掉 5 处 unused re-export，`Platform::new` 收进 `#[cfg(test)]`
- warning 从 27 条降到 14 条，剩下的都是 serde 反序列化字段（故意不读）与平台专属枚举变体

## 二进制编译验证

B12 这一轮真的编过了，不再只是静态审：

```bash
# 后端
cd src-tauri
cargo check          # 0 error，14 warning
cargo test --lib     # 144 passed / 0 failed

# 前端
npx vue-tsc --noEmit # 无输出即通过
npx vite build       # 94 modules，dist 正常产出
```

两个必须知道的坑：
1. **图标必须是 RGBA**。RGB 的 png/ico 会让 `generate_context!()` 在宏展开阶段 panic，只有真跑 `cargo check` 才会暴露。
2. **别在 `/sdcard` 里编译**。那块盘 noexec：cargo 会报 `fork: Function not implemented`，vite 会报 esbuild / rollup 的 `EACCES`。把项目复制到 `/root` 这类 ext4 路径再编，并且内存紧张时加 `-j2`。

## 开发环境

- Node.js 18+、pnpm 8+、Rust stable
- pnpm 10 起默认不执行依赖的 postinstall；首次 `pnpm install` 若提示 `Ignored build scripts: esbuild, vue-demi`，按它给的提示跑一次 `pnpm approve-builds` 全允许即可
- Windows：VS Build Tools（C++ 生成工具）+ WebView2 Runtime
- macOS：`xcode-select --install`

## 开发调试

```bash
pnpm install
pnpm tauri dev
```

`pnpm install` 会生成 `pnpm-lock.yaml`，请一并提交到版本库，保证依赖版本可复现。

## 目录结构

```
mclauncher/
├── src/
│   ├── api/            version / account / launch / install / loader / mods / modpack / crash（命令封装 + 错误文案归一）
│   ├── components/     TitleBar / SideNav / StatusBar
│   ├── pages/          Launch / Logs / Account / Downloads / Mods / Modpack / Crash / Multiplayer / Settings / About
│   ├── stores/         settings / notify / version / account / launch / install / loader / mods / modpack / crash
│   ├── styles/theme.css
│   ├── App.vue
│   └── main.ts
└── src-tauri/
    ├── src/
    │   ├── commands/    mod.rs / version.rs / account.rs / microsoft.rs / java.rs / launch.rs / install.rs / loader.rs / mods.rs / modpack.rs / crash.rs
    │   ├── account/     offline.rs / microsoft.rs / secret.rs
    │   ├── download/    file.rs / mirror.rs / plan.rs / pool.rs
    │   ├── install/     plan.rs / executor.rs
    │   ├── java/        probe.rs / runtime.rs
    │   ├── launch/      command.rs / proc.rs / logs.rs
    │   ├── loader/      meta.rs / install.rs
    │   ├── mods/        modrinth.rs / local.rs / install.rs
    │   ├── modpack/     mrpack.rs / install.rs（.mrpack 解析 / 实例导入与列表）
    │   ├── crash/       mod.rs（崩溃规则表 + 退出码解读）
    │   ├── minecraft/   manifest / version_json / rules / classpath / arguments / isolate / natives / inherit
    │   ├── error.rs     AppError 统一错误
    │   ├── fsutil.rs    目录定位 / 安全替换写 / 流式 SHA1
    │   ├── state.rs     全局配置、迁移、三种隔离模式
    │   ├── lib.rs
    │   └── main.rs
    ├── capabilities/default.json
    ├── icons/
    ├── Cargo.toml
    └── tauri.conf.json
```

## 构建产物

### Windows

```bash
pnpm tauri build --bundles nsis
```

产物：`src-tauri/target/release/bundle/nsis/Nova Launcher_0.1.0_x64-setup.exe`

未签名版本首次运行会被 SmartScreen 提示“未知发布者”，属正常现象，选“更多信息 → 仍要运行”。

### macOS Intel / Apple Silicon

```bash
rustup target add x86_64-apple-darwin
rustup target add aarch64-apple-darwin
pnpm tauri build --target x86_64-apple-darwin --bundles app
pnpm tauri build --target aarch64-apple-darwin --bundles app
```

产物：`src-tauri/target/<target>/release/bundle/macos/Nova Launcher.app`

重要说明：`tauri.conf.json` 里没有声明 CPU 架构的字段。“双架构支持”只能通过两套 `--target` 构建分别产出，或事后用 `lipo` 合并成 Universal 2。两个独立 .app 不等于一个 Universal 应用。

## macOS 签名说明

`tauri.conf.json` 设了 `bundle.macOS.signingIdentity = "-"`，即 ad-hoc 签名。

ad-hoc 签名能解决“完全未签名导致内核直接拒绝加载”的问题，但**不能保证在所有 macOS 版本上无提示双击打开**。若仍被 Gatekeeper 拦截：

```bash
xattr -dr com.apple.quarantine /path/to/Nova\ Launcher.app
```

正式分发必须做 Developer ID 签名 + 公证（配置 `APPLE_API_ISSUER`、`APPLE_API_KEY`、`APPLE_API_KEY_PATH`）。

## 配置文件位置

- Windows：`%APPDATA%\NovaLauncher\config.json`
- macOS：`~/Library/Application Support/NovaLauncher/config.json`
- 版本清单缓存：同目录下的 `version_manifest.json`

## 已知限制
1. 加载器与 Mod 的兼容只在搜索阶段用 facets 限制，装完不做事后校验；加载器升级未做（整合包导入已在 B9 支持）
2. 资源只直连官方：镜像的 assets 路由与官方不同形，国内网络下资源阶段可能偏慢
3. 1.6 之前的老版本用 `pre-1.6` 资源包，结构与现在的 asset index 不同，目前只保证能装上，不保证游戏内声音正常
4. native 包缺失不阻断启动，只给警告；无“一键修复依赖”按钮，靠重装 / 修复安装补齐
5. `preview_version_args` 需要本地已有版本 JSON，否则返回“尚未安装”
6. 联机仍是占位页
7. 微软登录只走设备码 + `consumers` 端点，支持个人微软账号；学校/企业账号（Azure AD 租户）未单独处理，可能报 XSTS 2148916238
8. 内置的 client id 是公开测试用途，自建 Azure 应用时用环境变量 `NOVA_MS_CLIENT_ID` 覆盖即可
9. 令牌只在启动前刷新一次，没有后台定时续期；长时间不启动后第一次启动会多花一次刷新时间
10. 皮肤与披风未接入，账户页只用彩色首字母方块代替头像
11. Forge / NeoForge 要跑官方安装器，本机必须有对应大版本的 Java；没 Java 时只提示去设置页，不会自动下运行时
12. 安装器自己会连 maven 下 processor 与库，走的是官方网络；国内网络下这一步可能很慢，目前没有镜像方案
13. Quilt 的 QSL 特殊处理、Forge 的 `--installServer` 分支都没做，只走客户端安装
14. 加载器安装没有取消按钮：安装器子进程一旦跑起来就不中断，只能等它自己退出
15. Mod 数据源只有 Modrinth，没接 CurseForge（后者要申请 API key，直连也不稳定）
16. 不自动安装前置：只在版本条目上提示“必需前置 N 个”，具体装哪个由玩家决定
17. 没有更新检查、批量升级与冲突分析；`mods/` 里已有同名文件直接跳过（只比文件名，不比内容）
18. 搜索与下载都直连 Modrinth 官方域名，没有镜像；国内网络可能需要代理
19. 整合包只认 Modrinth 的 `.mrpack`；CurseForge 的 zip（`manifest.json` + `overrides` 结构）结构完全不同，未支持
20. `modrinth.index.json` 的 `formatVersion` 大于 1 直接报错，不尝试向下兼容；`env` 只区分“纯服务端文件”一种情况
21. 整合包不做升级与更新检查：想更新只能重新导入，会另建一个 `名字-2` 实例，旧实例要手动删
22. 导入时单个文件下载失败不中断整包，只在日志与结果里计数；`overrides` 与 `client-overrides` 同名文件由后者直接覆盖，没有更细的 diff
23. 实例与加载器版本一一对应：导入时装的加载器版本写进 `instance.json` 的 `launch_id`，之后在下载页单独再装加载器不会自动改写它
24. 不要在 Android 的 `/sdcard` 里 `pnpm install`：那块盘既不能建符号链接、也没有执行权限，会报 `Failed to create symlink ... Permission denied (os error 13)`，即使装上，esbuild / rollup 的二进制也会 `EACCES`。要在设备内开发请把项目放到 `/root` 或 `/home` 这类 ext4 路径
25. 崩溃规则是一张静态表（12 条），只认报错关键词：没见过的报错、卡死、被任务管理器强杀、native 层不留日志的情况都分析不出来
26. 只分析日志尾部（默认 4000 行、上限 20000 行），崩溃点更靠前就得自己翻日志
27. 不去读 `hs_err_pid*.log` 与 Forge 单独生成的 crash-report 文件，只在建议里提一句让用户自己看
28. 启动前检查只看 client.jar、依赖库、natives 三样：assets 缺失、加载器父版本不存在、Java 版本不匹配这些仍然要等 JVM 自己报错
29. 安装页现在能显示缺几个文件、缺的是哪个，但补齐仍走整份「修复安装」：逐文件哈希校验重来一遍，慢但稳；整合包那侧的「补齐文件」只针对上次失败条目
30. SHA512 只在来源提供时才有（整合包索引给、Mojang 清单只给 sha1），所以 sha1 仍是主力；两边都缺时只按声明大小判断，此时无法防内容损坏
31. 安装三态只按文件是否存在判断，不校验哈希：显示「已完整」也可能内容已损坏，真正的校验发生在安装/修复时
32. 断点续传依赖服务器支持 `Range`：官方与 BMCLAPI 会给 206，个别 CDN 忽略 Range 只能整份重下（最多两轮）
33. 整合包「补齐文件」要求源 `.mrpack` 还在原路径；移动或删掉之后只能重新导入，会另建 `名字-2` 实例
34. `icon.icns` 仍是旧文件：Pillow 在 Linux 写不了现代 ICNS，macOS 打包前请用 `cargo tauri icon` 重新生成
35. 启动器窗口尺寸只在启动时应用一次；运行中手动拖窗口不会被记回配置，改尺寸要进设置页保存
36. 还剩 14 条 warning，都是 serde 反序列化后不读的字段与平台专属枚举变体，刻意保留
