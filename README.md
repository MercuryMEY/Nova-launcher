# Nova Launcher

基于 **Tauri 2 + Vue 3 + Rust** 的跨平台 Minecraft 启动器，功能对标 PCL2（非移植，完全重写）。
体积小、启动快，不依赖 Electron；核心逻辑全在 Rust 侧，前端只管界面。

## 功能一览

| 模块 | 能做什么 |
|---|---|
| 版本管理 | 从 Mojang 清单拉正式版 / 快照，一键安装；支持版本继承（`inheritsFrom`） |
| 加载器 | Fabric / Quilt / Forge / NeoForge 一键安装（Forge 系走官方安装器） |
| Java | 自动探测本机 Java 并读主版本；缺运行时可按需下载；启动前校验版本是否够用 |
| 账户 | 离线账户（UUID 双模式）；微软正版登录（设备码流程）；凭据加密存储 |
| 启动 | 完整还原官方启动参数（新旧两种格式），支持三种隔离模式、游戏分辨率与全屏三态 |
| 下载 | 官方 → BMCLAPI 自动回落、并发下载池、SHA1 校验、`.part` 断点续传、失败重试 |
| 安装状态 | 三态显示（未装 / 部分缺失 / 已完整），能列出缺哪些文件，缺的直接补齐 |
| Mod 管理 | Modrinth 搜索（按游戏版本 + 加载器筛选）、本地扫描、装 / 启停 / 删 |
| 整合包 | 导入 Modrinth `.mrpack`，自动建实例、装加载器、解 overrides；失败文件可单独补齐 |
| 崩溃分析 | 纯本地规则识别 12 类常见崩溃 + 退出码解读，给出中文原因与修复建议 |
| 界面 | 无边框自绘标题栏、明暗主题 + 多种强调色、状态栏统一通知 |

## 特点

- **纯 Rust 内核**：下载、解压、启动、哈希校验都在 Rust 侧跑，不经过 JS 桥，大文件也不卡界面
- **国内可用**：下载超时自动回落到 BMCLAPI 镜像，官方源慢也不会卡死
- **断电不怕**：配置原子写（临时文件 → `.bak` → 就位，失败自动回滚），下载支持续传，装到一半关掉能接着来
- **安全兜底**：白名单 CSP、解压大小与路径限额（防 zip 炸弹与路径穿越）、外链白名单、启动前完整性检查
- **可诊断**：启动日志实时回传界面；崩溃后直接读日志给出人话结论，不用自己翻 `latest.log`
- **装得干净**：三种隔离模式（完整隔离 / 版本隔离 / 不隔离），每个实例的 Mod 与存档互不污染
- **一套代码四处跑**：Windows（NSIS 安装包）、macOS（Intel + Apple Silicon）、Linux（deb / AppImage）

## 快速开始

```bash
pnpm install
pnpm tauri dev        # 开发
pnpm tauri build      # 打包
```

环境要求：Node.js 18+ / pnpm 8+ / Rust stable；Windows 需 VS C++ 生成工具 + WebView2，macOS 需 `xcode-select --install`。

> 构建细节、多平台打包、GitHub Actions 与踩坑记录见 [docs/BUILDING.md](docs/BUILDING.md)。

## 文档

- [docs/BUILDING.md](docs/BUILDING.md) — 开发环境、打包、GitHub Actions、编译踩坑
- [docs/LIMITATIONS.md](docs/LIMITATIONS.md) — 完整的已知限制清单（36 条）
- [docs/CHANGELOG.md](docs/CHANGELOG.md) — 各阶段（B1 ~ B12）交付内容

## 已知限制（摘要）

- **资源无镜像**：libraries / client 走 BMCLAPI，assets 只直连官方，国内资源阶段偏慢
- **只支持 Modrinth**：Mod 与整合包都不认 CurseForge；整合包只有 `.mrpack`，不支持升级，更新只能重导
- **无自动前置**：只在版本条目上提示「必需前置 N 个」，装哪个由你决定
- **校验有边界**：安装三态只看文件存不存在；Mojang 只给 sha1，来源没给哈希时只比对大小
- **已装即跳过**：`mods/` 里同名文件直接跳过，只比文件名不比内容
- **部分场景分析不出来**：崩溃分析是 12 条静态规则，只读日志尾部；卡死、强杀、native 层无日志看不到
- **启动器窗口尺寸**只在启动时应用一次，运行中拖窗不写回配置
- **`icon.icns` 待重生成**：macOS 打包前请先跑 `cargo tauri icon`
- **不要放在 `/sdcard` 开发**：那块盘 noexec + 不能建符号链接，编译与 `pnpm install` 都会失败

## 目录结构

```
mclauncher/
├── src/                   Vue 3 前端
│   ├── api/               命令封装（错误文案归一）
│   ├── components/        TitleBar / SideNav / StatusBar
│   ├── pages/             Launch / Downloads / Account / Mods / Modpack / Crash / Settings …
│   ├── stores/            Pinia 状态
│   └── styles/theme.css
└── src-tauri/             Rust 后端
    ├── src/commands/      对外命令（全部 async + spawn_blocking）
    ├── src/download/      镜像回落 / 并发池 / 校验 / 续传
    ├── src/install/       安装计划与执行器
    ├── src/launch/        参数组装 / 进程管理 / 日志 / 并发锁
    ├── src/loader/        加载器元数据与安装
    ├── src/mods/          Modrinth / 本地扫描 / 安装
    ├── src/modpack/       .mrpack 解析与实例导入
    ├── src/crash/         崩溃规则表
    └── src/minecraft/     清单 / 版本 JSON / rules / classpath / natives / 隔离
```

## 配置文件

- Windows：`%APPDATA%\NovaLauncher\config.json`
- macOS：`~/Library/Application Support/NovaLauncher/config.json`
- Linux：`~/.config/NovaLauncher/config.json`

## 许可

仅供学习交流。Minecraft 是 Mojang Studios 的商标，本项目与其无关联。