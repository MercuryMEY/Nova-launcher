import { invoke } from "@tauri-apps/api/core";

/** 四种加载器的静态信息，后端写死一份，前端只负责展示 */
export interface LoaderKindInfo {
  key: string;
  label: string;
  /** 要跑官方 installer（Forge / NeoForge），会多花几分钟 */
  needs_installer: boolean;
  note: string;
}

export interface LoaderVersionInfo {
  /** 加载器自己的版本号，例如 0.16.5 / 47.2.0 / 21.1.9 */
  id: string;
  /** 对应的 MC 版本，避免前端串台 */
  game: string;
  stable: boolean;
  note: string;
}

export interface LoaderSummary {
  /** 装好后的版本目录名，启动页拿它启动 */
  id: string;
  kind: string;
  version: string;
  downloaded: number;
  skipped: number;
  seconds: number;
}

export interface InstalledLoaderView {
  id: string;
  /** 从目录名认出来的类型，认不出是 null */
  kind: string | null;
  loader_version: string | null;
}

export interface LoaderStagePayload {
  stage: string;
}

export interface LoaderProgressPayload {
  done: number;
  total: number;
}

export interface LoaderLogPayload {
  line: string;
}

export interface LoaderDonePayload {
  id: string;
  kind: string;
  version: string;
  downloaded: number;
  skipped: number;
  seconds: number;
}

/** 错误提示文案与版本页共用一份 */
export { errorText } from "./version";

export const loaderApi = {
  kinds: () => invoke<LoaderKindInfo[]>("loader_kinds"),
  versions: (game: string, kind: string) =>
    invoke<LoaderVersionInfo[]>("loader_versions", { game, kind }),
  install: (game: string, kind: string, version: string) =>
    invoke<LoaderSummary>("loader_install", { game, kind, version }),
  installed: (game: string) => invoke<InstalledLoaderView[]>("loader_installed", { game }),
};
