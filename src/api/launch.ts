import { invoke } from "@tauri-apps/api/core";

export interface JavaEntry {
  path: string;
  version: string;
  major: number;
  arch: string;
  source: string;
}

export interface JavaStatus {
  /** 这个版本要求的 Java 大版本 */
  target: number;
  /** 版本 JSON 点名的运行时组件，仅作展示 */
  component: string | null;
  /** 当前会被选中的那个，null 表示跑不起来 */
  chosen: string | null;
  detected: JavaEntry[];
}

export interface JavaInstallResult {
  java: string;
  component: string;
  major: number;
}

export interface LaunchSummary {
  version_id: string;
  pid: number;
  java: string;
  log_path: string;
  natives_extracted: number;
  natives_skipped: number;
  natives_missing: number;
  missing_files: number;
}

export interface ExitInfo {
  code: number | null;
  success: boolean;
  seconds: number;
}

export interface GameStatus {
  running: boolean;
  pid: number;
  version_id: string;
  java: string;
  started_at: string;
  log_path: string;
  exit: ExitInfo | null;
}

export interface LogFileInfo {
  name: string;
  size: number;
  modified: string;
}

export interface LogTail {
  lines: string[];
  truncated: boolean;
}

/** 事件 payload，和后端 emit 的字段一字对齐 */
export interface StagePayload {
  stage: string;
}

export interface LogPayload {
  id: string;
  line: string;
  stream: "stdout" | "stderr";
}

export interface ExitPayload {
  id: string;
  code: number | null;
  success: boolean;
  seconds: number;
  log_path: string;
}

export interface JavaProgressPayload {
  stage: string;
  done?: number;
  total?: number;
}

/** 复用版本页的错误归一化，提示文案只维护一份 */
export { errorText } from "./version";

export const launchApi = {
  start: (id: string, instance?: string) =>
    invoke<LaunchSummary>("launch_game", { id, instance: instance ?? null }),
  stop: () => invoke<void>("stop_game"),
  status: () => invoke<GameStatus | null>("game_status"),
  listJava: (force = false) => invoke<JavaEntry[]>("list_java", { force }),
  javaStatus: (id: string) => invoke<JavaStatus>("java_status", { id }),
  installJava: (id: string) => invoke<JavaInstallResult>("install_java_runtime", { id }),
  logs: () => invoke<LogFileInfo[]>("list_logs"),
  readLog: (name: string, lines = 500) => invoke<LogTail>("read_log", { name, lines }),
  deleteLog: (name: string) => invoke<void>("delete_log", { name }),
};
