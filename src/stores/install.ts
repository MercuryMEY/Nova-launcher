import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import {
  errorText,
  installApi,
  type InstallDonePayload,
  type InstallPreview,
  type InstallProgressPayload,
  type InstallStagePayload,
  type InstallSummary,
} from "../api/install";
import { useNotifyStore } from "./notify";

export const useInstallStore = defineStore("install", () => {
  const notify = useNotifyStore();

  const preview = ref<InstallPreview | null>(null);
  const previewing = ref(false);
  const running = ref(false);
  const cancelling = ref(false);
  /** 当前阶段的文案，例如“正在下载依赖库” */
  const stage = ref("");
  const done = ref(0);
  const total = ref(0);
  /** 每个阶段各自的总数，进度条上要显示“第几批” */
  const stagePlan = ref<Record<string, number>>({});
  const last = ref<InstallSummary | null>(null);

  const percent = computed(() => {
    if (total.value <= 0) return 0;
    return Math.min(100, Math.round((done.value / total.value) * 100));
  });

  let bound = false;

  /** 事件只绑一次，否则进度会在同一个百分比上来回跳 */
  function bind() {
    if (bound) return;
    bound = true;

    void listen<InstallStagePayload>("install:stage", (e) => {
      stage.value = e.payload.stage;
      done.value = 0;
      total.value = 0;
    });

    void listen<InstallProgressPayload>("install:progress", (e) => {
      const p = e.payload;
      stage.value = p.stage;
      done.value = p.done;
      total.value = p.total;
    });

    void listen<InstallDonePayload>("install:done", (e) => {
      const p = e.payload;
      stage.value = "";
      done.value = 0;
      total.value = 0;
      if (p.skipped > 0) {
        notify.info(`${p.id} 装好了，${p.skipped} 个文件本来就存在`);
      }
    });
  }

  /** 预览：只拉计划，不落盘 */
  async function loadPreview(id: string) {
    previewing.value = true;
    try {
      const p = await installApi.preview(id);
      preview.value = p;
      stagePlan.value = Object.fromEntries(p.stages.map((s) => [s.stage, s.files]));
      return p;
    } catch (e) {
      preview.value = null;
      notify.error(errorText(e));
      return null;
    } finally {
      previewing.value = false;
    }
  }

  async function start(id: string): Promise<InstallSummary | null> {
    if (running.value) return null;
    running.value = true;
    cancelling.value = false;
    last.value = null;
    stage.value = "正在准备";
    done.value = 0;
    total.value = 0;
    try {
      const r = await installApi.start(id);
      last.value = r;
      notify.success(
        `${id} 安装完成：新下 ${r.downloaded} 个，跳过 ${r.skipped} 个，用了 ${r.seconds} 秒`
      );
      // 装完刷新一次预览，按钮会从“安装”变“修复”
      await loadPreview(id);
      return r;
    } catch (e) {
      notify.error(errorText(e));
      return null;
    } finally {
      running.value = false;
      stage.value = "";
      cancelling.value = false;
    }
  }

  async function cancel() {
    if (!running.value || cancelling.value) return;
    cancelling.value = true;
    try {
      const ok = await installApi.cancel();
      notify.info(ok ? "正在停止下载" : "当前没有可取消的任务");
    } catch (e) {
      notify.error(errorText(e));
    }
  }

  return {
    preview,
    previewing,
    running,
    cancelling,
    stage,
    done,
    total,
    percent,
    stagePlan,
    last,
    bind,
    loadPreview,
    start,
    cancel,
  };
});
