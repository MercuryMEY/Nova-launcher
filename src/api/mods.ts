import { invoke } from "@tauri-apps/api/core";

/** Modrinth 搜索结果的一条。字段名跟后端保持一致（Rust 侧不做 camelCase 转换） */
export interface ModHit {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  author: string;
  downloads: number;
  follows: number;
  icon_url: string | null;
  categories: string[];
  /** 这个项目支持哪些 MC 版本 */
  game_versions: string[];
}

export interface ModSearchPage {
  hits: ModHit[];
  total: number;
  offset: number;
}

/** 一个版本挂的可下载文件，后端已经挑好 primary 了 */
export interface ModFile {
  filename: string;
  url: string;
  sha1: string;
  size: number;
}

export interface ModDependency {
  project_id: string | null;
  version_id: string | null;
  /** required / optional / incompatible / embedded */
  dependency_type: string;
}

export interface ModVersion {
  id: string;
  name: string;
  version_number: string;
  game_versions: string[];
  loaders: string[];
  /** release / beta / alpha */
  version_type: string;
  downloads: number;
  date_published: string;
  file: ModFile;
  dependencies: ModDependency[];
}

/** mods 目录里的一个 jar，元数据读不到时 id / name 为 null */
export interface LocalMod {
  file_name: string;
  path: string;
  enabled: boolean;
  id: string | null;
  name: string | null;
  version: string | null;
  loader: string | null;
  size: number;
}

export interface ModInstallResult {
  path: string;
  /** 同名校验通过，没有真下载 */
  skipped: boolean;
  dir: string;
}

export type SortKey = "relevance" | "downloads" | "follows" | "newest" | "updated";

/** 错误文案统一走版本页那份，避免两处写法漂移 */
export { errorText } from "./version";

/** 从版本目录名猜加载器。认不出返回空串，界面上就是「不限」 */
export function detectLoader(id: string): string {
  const s = id.toLowerCase();
  if (s.includes("fabric-loader")) return "fabric";
  if (s.includes("quilt-loader")) return "quilt";
  if (s.includes("neoforge")) return "neoforge";
  if (s.includes("forge")) return "forge";
  return "";
}

/**
 * 从版本目录名还原出对应的原版版本号。
 *
 * Fabric / Quilt 的目录名是 `fabric-loader-0.16.5-1.21.4`，MC 版本在最后一段；
 * Forge / NeoForge 是 `1.21.4-forge-54.1.0`，MC 版本在第一段。
 * 只覆盖正式版，快照版上装 Mod 本身就极罕见。
 */
export function baseGameOf(id: string): string {
  const loader = detectLoader(id);
  if (loader === "fabric" || loader === "quilt") {
    const parts = id.split("-");
    return parts[parts.length - 1] ?? id;
  }
  return id.split("-")[0] ?? id;
}

export const LOADER_LABELS: Record<string, string> = {
  fabric: "Fabric",
  quilt: "Quilt",
  forge: "Forge",
  neoforge: "NeoForge",
};

export const modsApi = {
  search: (
    game: string,
    loader: string,
    query: string,
    sort: SortKey,
    offset: number,
    limit: number
  ) =>
    invoke<ModSearchPage>("mods_search", { game, loader, query, sort, offset, limit }),
  versions: (projectId: string, game: string, loader: string) =>
    invoke<ModVersion[]>("mods_versions", { projectId, game, loader }),
  /** instance 有值时后端把 mods 目录指向 instances/<id>/mods，此时 game 只是搜索用的版本号 */
  install: (game: string, instance: string | null, file: ModFile) =>
    invoke<ModInstallResult>("mods_install", { game, instance, file }),
  list: (game: string, instance: string | null) =>
    invoke<LocalMod[]>("mods_list", { game, instance }),
  dir: (game: string, instance: string | null) =>
    invoke<string>("mods_dir_of", { game, instance }),
  toggle: (game: string, instance: string | null, fileName: string, enable: boolean) =>
    invoke<string>("mods_toggle", { game, instance, fileName, enable }),
  remove: (game: string, instance: string | null, fileName: string) =>
    invoke<void>("mods_remove", { game, instance, fileName }),
};
