import { defineStore } from "pinia";
import { ref } from "vue";
import { crashApi, type CrashAnalysis } from "../api/crash";
import { errorText } from "../api/version";
import { useNotifyStore } from "./notify";

export const useCrashStore = defineStore("crash", () => {
  const notify = useNotifyStore();

  const report = ref<CrashAnalysis | null>(null);
  const analyzing = ref(false);
  /** 这次分析的是哪份日志，空字符串表示「最新的那份」 */
  const source = ref("");

  async function analyze(name?: string | null, code?: number | null) {
    if (analyzing.value) return null;
    analyzing.value = true;
    try {
      const r = await crashApi.analyze(name, code);
      report.value = r;
      source.value = name ?? "";
      return r;
    } catch (e) {
      // 没有日志、文件读不了都走这里，提示已经够长，不再额外弹窗
      notify.error(`分析失败：${errorText(e)}`);
      return null;
    } finally {
      analyzing.value = false;
    }
  }

  function clear() {
    report.value = null;
    source.value = "";
  }

  return { report, analyzing, source, analyze, clear };
});