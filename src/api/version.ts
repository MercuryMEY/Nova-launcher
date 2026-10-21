import { invoke } from "@tauri-apps/api/core";

export interface RemoteVersion {
  id: string;
  kind: string;
  release_time: string;
  installed: boolean;
}

export interface RemoteVersions {
  latest_release: string;
  latest_snapshot: string;
  /** official / mirror / cache */
  source: string;
  cached: boolean;
  versions: RemoteVersion[];
}

export interface InstalledVersion {
  id: string;
  path: string;
  java_major: number | null;
  has_json: boolean;
  isolated: boolean;
}

export interface VersionPreview {
  id: string;
  main_class: string;
  java_major: number | null;
  asset_index: string | null;
  game_dir: string;
  libraries: number;
  natives: number;
  client_jar_exists: boolean;
  missing_files: string[];
  jvm_args: string[];
  game_args: string[];
}

/** Rust 侧的 AppError 统一序列化成 { code, message } */
export interface AppErrorPayload {
  code: string;
  message: string;
}

/** 把任意异常转成能直接展示给玩家的一句话 */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  const payload = e as AppErrorPayload | null;
  if (payload && typeof payload.message === "string" && payload.message) {
    return payload.message;
  }
  return "操作失败了，可以重试一次看看";
}

export const versionApi = {
  remote: () => invoke<RemoteVersions>("list_remote_versions"),
  installed: () => invoke<InstalledVersion[]>("list_installed_versions"),
  preview: (id: string) => invoke<VersionPreview>("preview_version_args", { id }),
};
