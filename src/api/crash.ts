import { invoke } from "@tauri-apps/api/core";

/** 与后端 Severity 一一对应 */
export type Severity = "fatal" | "warn" | "info";

export interface Evidence {
  /** 行号，相对被分析的那段日志（1 开始） */
  line: number;
  text: string;
}

export interface Finding {
  /** 规则 id，例如 out_of_memory，前端只用它做 key */
  id: string;
  severity: Severity;
  title: string;
  detail: string;
  /** 最多 3 条 */
  evidence: Evidence[];
  advice: string[];
}

export interface CrashAnalysis {
  /** 一句话结论 */
  headline: string;
  severity: Severity;
  /** 实际扫了多少行 */
  scanned: number;
  /** 日志太大，只看了尾部 */
  truncated: boolean;
  findings: Finding[];
  /** 退出码的通俗解读 */
  exit_hint: string | null;
}

export const SEVERITY_LABELS: Record<Severity, string> = {
  fatal: "致命",
  warn: "可疑",
  info: "提示",
};

export const crashApi = {
  /** name 为空时后端取最新那份日志 */
  analyze: (name?: string | null, code?: number | null) =>
    invoke<CrashAnalysis>("crash_analyze", {
      name: name ?? null,
      code: code ?? null,
      lines: null,
    }),
};