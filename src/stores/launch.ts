import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import {
  errorText,
  launchApi,
  type ExitPayload,
  type GameStatus,
  type JavaEntry,
  type JavaProgressPayload,
  type JavaStatus,
  type LaunchSummary,
  type LogFileInfo,
  type LogPayload,
  type StagePayload,
} from "../api/launch";
import { useNotifyStore } from "./notify";

/** 实时输出只在内存里留这么多行，游戏跑久了也不会把内存吃干 */
const LIVE_LIMIT = 2000;

export const useLaunchStore = defineStore("launch", () => {
  const notify = useNotifyStore();

  const status = ref<GameStatus | null>(null);
  const launching = ref(false);
  const stopping = ref(false);
  /** 启动过程中后端传来的"当前在做什么" */
  const stage = ref("");
  const live = ref<string[]>([]);
  /** 上一次启动失败的原文：界面据此判断要不要给「去修复」入口 */
  const lastError = ref("");

  const javas = ref<JavaEntry[]>([]);
  const javaLoading = ref(false);
  const javaStatus = ref<JavaStatus | null>(null);
  const installing = ref(false);
  const installStage = ref("");
  const installPercent = ref(0);

  const logFiles = ref<LogFileInfo[]>([]);

  const running = computed(() => status.value?.running === true);

  let bound = false;

  /** 事件只绑一次：重复绑定会让每行日志进两次 */
  function bind() {
    if (bound) return;
    bound = true;

    void listen<LogPayload>("game:log", (e) => {
      live.value.push(e.payload.line);
      if (live.value.length > LIVE_LIMIT) {
        live.value.splice(0, live.value.length - LIVE_LIMIT);
      }
    });

    void listen<StagePayload>("launch:stage", (e) => {
      stage.value = e.payload.stage;
    });

    void listen<ExitPayload>("game:exit", (e) => {
      const p = e.payload;
      stage.value = "";
      if (p.success) {
        notify.info(`${p.id} 已退出（${p.seconds.toFixed(0)} 秒）`);
      } else {
        notify.error(`${p.id} 异常退出，退出码 ${p.code ?? "未知"}，日志已保存`);
      }
      void refresh();
      void loadLogs();
    });

    void listen<JavaProgressPayload>("java:progress", (e) => {
      const p = e.payload;
      installStage.value = p.stage;
      if (typeof p.done === "number" && typeof p.total === "number" && p.total > 0) {
        installPercent.value = Math.min(100, Math.round((p.done / p.total) * 100));
      }
    });
  }

  async function refresh() {
    try {
      status.value = await launchApi.status();
    } catch {
      status.value = null;
    }
  }

  async function loadJava(id: string) {
    javaLoading.value = true;
    try {
      javaStatus.value = await launchApi.javaStatus(id);
    } catch {
      javaStatus.value = null;
    } finally {
      javaLoading.value = false;
    }
  }

  async function listJava(force = false) {
    javaLoading.value = true;
    try {
      javas.value = await launchApi.listJava(force);
    } catch (e) {
      notify.error(errorText(e));
    } finally {
      javaLoading.value = false;
    }
  }

  async function installJava(id: string) {
    if (installing.value) return null;
    installing.value = true;
    installPercent.value = 0;
    installStage.value = "正在准备";
    try {
      const r = await launchApi.installJava(id);
      notify.success(`Java ${r.major} 已装好`);
      await Promise.all([listJava(true), loadJava(id)]);
      return r;
    } catch (e) {
      notify.error(errorText(e));
      return null;
    } finally {
      installing.value = false;
      installStage.value = "";
    }
  }

  /** instance 只在启动整合包实例时传：把 gameDir 指到 instances/<id> */
  async function launch(id: string, instance?: string): Promise<LaunchSummary | null> {
    if (launching.value || running.value) return null;
    launching.value = true;
    lastError.value = "";
    live.value = [];
    stage.value = "正在准备启动";
    try {
      const summary = await launchApi.start(id, instance);
      stage.value = "";
      const parts = [`${summary.version_id} 已启动`];
      if (summary.natives_extracted > 0) parts.push(`解压 natives ${summary.natives_extracted} 个`);
      notify.success(parts.join("，"));
      await refresh();
      void loadLogs();
      return summary;
    } catch (e) {
      stage.value = "";
      lastError.value = errorText(e);
      notify.error(lastError.value);
      return null;
    } finally {
      launching.value = false;
    }
  }

  async function stop() {
    if (stopping.value) return;
    stopping.value = true;
    try {
      await launchApi.stop();
      notify.info("已发送结束信号");
      await refresh();
    } catch (e) {
      notify.error(errorText(e));
    } finally {
      stopping.value = false;
    }
  }

  async function loadLogs() {
    try {
      logFiles.value = await launchApi.logs();
    } catch {
      logFiles.value = [];
    }
  }

  async function readLog(name: string, lines = 800) {
    try {
      return await launchApi.readLog(name, lines);
    } catch (e) {
      notify.error(errorText(e));
      return null;
    }
  }

  async function deleteLog(name: string) {
    try {
      await launchApi.deleteLog(name);
      await loadLogs();
      notify.success("日志已删除");
    } catch (e) {
      notify.error(errorText(e));
    }
  }

  return {
    status,
    launching,
    stopping,
    stage,
    live,
    lastError,
    running,
    javas,
    javaLoading,
    javaStatus,
    installing,
    installStage,
    installPercent,
    logFiles,
    bind,
    refresh,
    loadJava,
    listJava,
    installJava,
    launch,
    stop,
    loadLogs,
    readLog,
    deleteLog,
  };
});
