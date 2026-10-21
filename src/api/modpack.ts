import { invoke } from "@tauri-apps/api/core";

/** 选中 .mrpack 后的预览信息，导入前给用户看一眼 */
export interface PackPreview {
  /** 将要创建的实例目录名，重名时已经带上了 `-2` 这类后缀 */
  id: string;
  name: string;
  summary: string;
  game: string;
  /** fabric / quilt / forge / neoforge，包里没声明就是 null */
  loader: string | null;
  loader_label: string | null;
  loader_version: string | null;
  file_count: number;
  total_size: number;
  dir: string;
  /** 已经有同名实例，这次会另存一份 */
  existing: boolean;
  /** 对应原版装没装；没装的话加载器那步会被跳过 */
  base_installed: boolean;
}

/** `instances/<id>/instance.json` 的内容，字段跟后端一字对齐 */
export interface ModpackInstance {
  id: string;
  name: string;
  summary: string;
  game: string;
  loader: string | null;
  loader_version: string | null;
  /** 启动用的版本目录名；加载器没装成时是 null，此时暂时启动不了 */
  launch_id: string | null;
  /** Unix 秒，字符串形式 */
  created: string;
  file_count: number;
  /** 上次导入没下下来的条目，空数组表示这次是全的 */
  failed: string[];
  /** 源整合包路径，重试缺失文件要用 */
  source: string | null;
}

export interface ModpackImportResult {
  instance: ModpackInstance;
  downloaded: number;
  skipped: number;
  failed: number;
  overrides: number;
  launch_id: string | null;
  seconds: number;
}

export interface ModpackStagePayload {
  stage: string;
}

/** 导入阶段单位是字节，加载器阶段也是字节（后端已经统一），直接拿来算百分比 */
export interface ModpackProgressPayload {
  done: number;
  total: number;
}

export interface ModpackLogPayload {
  line: string;
}

export interface ModpackDonePayload {
  id: string;
}

/** 后端当前能解析的 `modrinth.index.json` 格式版本 */
export const SUPPORTED_FORMAT = 1;

export const modpackApi = {
  inspect: (path: string) => invoke<PackPreview>("modpack_inspect", { path }),
  import: (path: string, installLoader: boolean) =>
    invoke<ModpackImportResult>("modpack_import", { path, installLoader }),
  list: () => invoke<ModpackInstance[]>("modpack_list"),
  /** 只补上次没下下来的文件，不重解压 overrides */
  retry: (id: string) => invoke<ModpackInstance>("modpack_retry", { id }),
  remove: (id: string) => invoke<void>("modpack_remove", { id }),
  format: () => invoke<number>("modpack_format"),
};
