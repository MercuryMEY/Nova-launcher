import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import {
  errorText,
  loaderApi,
  type InstalledLoaderView,
  type LoaderDonePayload,
  type LoaderKindInfo,
  type LoaderLogPayload,
  type LoaderProgressPayload,
  type LoaderStagePayload,
  type LoaderSummary,
  type LoaderVersionInfo,
} from "../api/loader";
import { useNotifyStore } from "./notify";

/** 安装器输出可能上千行，界面只留最后这些，多了滚都滚不完 */
const LOG_LIMIT = 200;

export const useLoaderStore = defineStore("loader", () => {
  const notify = useNotifyStore();

  /** 这批列表是挂在哪个原版版本下的，切版本时用来判断要不要重拉 */
  const game = ref("");
  const kinds = ref<LoaderKindInfo[]>([]);
  /** 当前挑中的加载器类型；默认 Fabric，装的人最多 */
  const kind = ref("fabric");
  const versions = ref<LoaderVersionInfo[]>([]);
  const versionId = ref("");
  const loadingVersions = ref(false);
  const installing = ref(false);
  const stage = ref("");
  const done = ref(0);
  const total = ref(0);
  const logs = ref<string[]>([]);
  const last = ref<LoaderSummary | null>(null);
  /** 当前选中的原版下已经装好的加载器 */
  const installed = ref<InstalledLoaderView[]>([]);

  const percent = computed(() => {
    if (total.value <= 0) return 0;
    return Math.min(100, Math.round((done.value / total.value) * 100));
  });

  const currentKind = computed(() => kinds.value.find((k) => k.key === kind.value) ?? null);

  /** 有版本列表就应该有默认选中项，否则按钮永远是灰的 */
  function pickDefaultVersion() {
    if (!versions.value.length) {
      versionId.value = "";
      return;
    }
    if (versions.value.some((v) => v.id === versionId.value)) return;
    const stable = versions.value.find((v) => v.stable);
    versionId.value = (stable ?? versions.value[0]).id;
  }

  let bound = false;

  /** 事件只绑一次，重复绑会让进度条来回跳 */
  function bind() {
    if (bound) return;
    bound = true;

    void listen<LoaderStagePayload>("loader:stage", (e) => {
      stage.value = e.payload.stage;
      done.value = 0;
      total.value = 0;
    });

    void listen<LoaderProgressPayload>("loader:progress", (e) => {
      done.value = e.payload.done;
      total.value = e.payload.total;
    });

    void listen<LoaderLogPayload>("loader:log", (e) => {
      logs.value.push(e.payload.line);
      if (logs.value.length > LOG_LIMIT) {
        logs.value.splice(0, logs.value.length - LOG_LIMIT);
      }
    });

    void listen<LoaderDonePayload>("loader:done", (e) => {
      stage.value = "";
      done.value = 0;
      total.value = 0;
    });
  }

  async function loadKinds() {
    if (kinds.value.length) return kinds.value;
    try {
      kinds.value = await loaderApi.kinds();
      if (!kinds.value.some((k) => k.key === kind.value) && kinds.value.length) {
        kind.value = kinds.value[0].key;
      }
      return kinds.value;
    } catch (e) {
      notify.error(errorText(e));
      return kinds.value;
    }
  }

  async function loadInstalled(game: string) {
    if (!game) {
      installed.value = [];
      return;
    }
    try {
      installed.value = await loaderApi.installed(game);
    } catch {
      // 已装清单只是锦上添花，读不到就当没有，不打扰玩家
      installed.value = [];
    }
  }

  /** 切换加载器类型要重拉版本列表，因为各家的版本号体系完全不同 */
  async function loadVersions(game: string, k: string) {
    if (!game) return;
    kind.value = k;
    loadingVersions.value = true;
    try {
      versions.value = await loaderApi.versions(game, k);
      pickDefaultVersion();
    } catch (e) {
      versions.value = [];
      versionId.value = "";
      notify.error(errorText(e));
    } finally {
      loadingVersions.value = false;
    }
  }

  /** 切原版版本时把上一份状态清掉，不然会看到别人家的版本号 */
  async function switchGame(next: string) {
    // 同一个原版版本反复点不用重拉，元数据接口也要省着用
    if (game.value === next && versions.value.length > 0) return;
    game.value = next;
    installing.value = false;
    stage.value = "";
    done.value = 0;
    total.value = 0;
    logs.value = [];
    last.value = null;
    versions.value = [];
    versionId.value = "";
    await loadInstalled(next);
    if (next) await loadVersions(next, kind.value);
  }

  async function install(base: string): Promise<LoaderSummary | null> {
    if (installing.value) return null;
    if (!base || !versionId.value) {
      notify.info("先选一个加载器版本");
      return null;
    }
    installing.value = true;
    stage.value = "正在准备";
    done.value = 0;
    total.value = 0;
    logs.value = [];
    last.value = null;
    try {
      const r = await loaderApi.install(base, kind.value, versionId.value);
      last.value = r;
      notify.success(`${r.id} 装好了，用了 ${r.seconds} 秒`);
      await loadInstalled(base);
      return r;
    } catch (e) {
      notify.error(errorText(e));
      return null;
    } finally {
      installing.value = false;
      stage.value = "";
    }
  }

  return {
    game,
    kinds,
    kind,
    currentKind,
    versions,
    versionId,
    loadingVersions,
    installing,
    stage,
    done,
    total,
    percent,
    logs,
    last,
    installed,
    bind,
    loadKinds,
    loadInstalled,
    loadVersions,
    switchGame,
    install,
  };
});