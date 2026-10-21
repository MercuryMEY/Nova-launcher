import { invoke } from "@tauri-apps/api/core";

/** 三态：完全没装 / 装了但缺文件 / 完整 */
export type InstallStatus = "missing" | "partial" | "complete";

export interface StageCount {
  stage: string;
  files: number;
}

export interface InstallPreview {
  id: string;
  has_client: boolean;
  libraries: number;
  assets: number;
  has_logging: boolean;
  total_files: number;
  total_bytes: number;
  /** 三态判断，不再只看 client.jar */
  status: InstallStatus;
  missing_count: number;
  /** 最多 20 个缺失文件名 */
  missing_names: string[];
  stages: StageCount[];
}

export interface InstallSummary {
  downloaded: number;
  skipped: number;
  bytes: number;
  seconds: number;
  total_files: number;
}

export interface InstallStagePayload {
  stage: string;
}

export interface InstallProgressPayload {
  stage: string;
  done: number;
  total: number;
}

export interface InstallDonePayload {
  id: string;
  downloaded: number;
  skipped: number;
  seconds: number;
}

/** 复用版本页的错误归一化，提示文案只维护一份 */
export { errorText } from "./version";

export const installApi = {
  preview: (id: string) => invoke<InstallPreview>("preview_install", { id }),
  start: (id: string) => invoke<InstallSummary>("start_install", { id }),
  cancel: () => invoke<boolean>("cancel_install"),
  busy: () => invoke<boolean>("install_busy"),
};
