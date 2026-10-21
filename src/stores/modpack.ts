import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  modpackApi,
  type ModpackDonePayload,
  type ModpackImportResult,
  type ModpackInstance,
  type ModpackLogPayload,
  type ModpackProgressPayload,
  type ModpackStagePayload,
  type PackPreview,
} from "../api/modpack";
import { errorText } from "../api/version";
import { useNotifyStore } from "./notify";

/** 日志留最后这么多行，整合包动辄几十个文件，全留着会拖慢渲染 */
const LOG_LIMIT = 400;

export const useModpackStore = defineStore("modpack", () => {
  const notify = useNotifyStore();

  /** 选中的包，还没导入 */
  const preview = ref<PackPreview | null>(null);
  /** 选中的 .mrpack 绝对路径，导入时要原样传给后端 */
  const zipPath = ref("");
  const inspecting = ref(false);
  const importing = ref(false);
  /** 正在补缺失文件的实例 id，空串表示没在补 */
  const retrying = ref("");
  const stage = ref("");
  const done = ref(0);
  const total = ref(0);
  const logs = ref<string[]>([]);
  const instances = ref<ModpackInstance[]>([]);
  const last = ref<ModpackImportResult | null>(null);

  const percent = computed(() => {
    if (total.value <= 0) return 0;
    return Math.min(100, Math.round((done.value / total.value) * 100));
  });

  let bound = false;
  /** 事件只绑一次，重复绑会让同一条日志出现两遍 */
  function bind() {
    if (bound) return;
    bound = true;
    void listen<ModpackStagePayload>("modpack:stage", (e) => {
      stage.value = e.payload.stage;
      // 换了阶段就重置进度，否则会从上一阶段的 80% 掉回 0 之前先跳一下
      done.value = 0;
      total.value = 0;
      pushLog(`—— ${e.payload.stage} ——`);
    });
    void listen<ModpackProgressPayload>("modpack:progress", (e) => {
      done.value = e.payload.done;
      total.value = e.payload.total;
    });
    void listen<ModpackLogPayload>("modpack:log", (e) => {
      pushLog(e.payload.line);
    });
    void listen<ModpackDonePayload>("modpack:done", () => {
      stage.value = "";
      done.value = 0;
      total.value = 0;
    });
  }

  function pushLog(line: string) {
    logs.value.push(line);
    if (logs.value.length > LOG_LIMIT) {
      logs.value.splice(0, logs.value.length - LOG_LIMIT);
    }
  }

  /** 弹系统文件框选 .mrpack；用户点取消返回 null */
  async function pick(): Promise<string | null> {
    try {
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Modrinth 整合包", extensions: ["mrpack"] }],
      });
      if (!picked || Array.isArray(picked)) return null;
      return picked;
    } catch (e) {
      notify.error(`打不开文件选择框：${errorText(e)}`);
      return null;
    }
  }

  /** 选中后立刻预览，把包名、加载器、体积这些先摆出来 */
  async function inspect(path: string) {
    inspecting.value = true;
    preview.value = null;
    logs.value = [];
    last.value = null;
    try {
      const p = await modpackApi.inspect(path);
      preview.value = p;
      zipPath.value = path;
      // 索引格式比后端支持的新时，后端会直接报错，这里只是一句提前提醒
      return p;
    } catch (e) {
      notify.error(`这个包读不了：${errorText(e)}`);
      return null;
    } finally {
      inspecting.value = false;
    }
  }

  /** 真正开跑：解包 → 铺 overrides → 下载 → 写 instance.json → 可能装加载器 */
  async function importPack(installLoader: boolean) {
    const p = preview.value;
    if (!p || importing.value) return null;
    importing.value = true;
    logs.value = [];
    done.value = 0;
    total.value = 0;
    try {
      const r = await modpackApi.import(zipPath.value, installLoader);
      last.value = r;
      await refresh();
      if (r.failed > 0) {
        // 少数文件下载失败不算整体失败，文件能用的都在盘上了
        notify.info(`${r.instance.name} 导入完成，${r.failed} 个文件没下下来`);
      } else {
        notify.success(`${r.instance.name} 导入完成，用时 ${r.seconds}s`);
      }
      preview.value = null;
      return r;
    } catch (e) {
      notify.error(`导入失败：${errorText(e)}`);
      return null;
    } finally {
      importing.value = false;
      stage.value = "";
      done.value = 0;
      total.value = 0;
    }
  }

  async function refresh() {
    try {
      instances.value = await modpackApi.list();
    } catch (e) {
      notify.error(`读取实例列表失败：${errorText(e)}`);
    }
  }

  /** 只补缺失文件：不解压 overrides，也不重装加载器 */
  async function retry(id: string) {
    if (retrying.value) return null;
    retrying.value = id;
    done.value = 0;
    total.value = 0;
    try {
      const inst = await modpackApi.retry(id);
      await refresh();
      if (inst.failed.length) {
        notify.info(`${inst.name} 还有 ${inst.failed.length} 个文件没下下来`);
      } else {
        notify.success(`${inst.name} 的缺失文件已补齐`);
      }
      return inst;
    } catch (e) {
      notify.error(`重试失败：${errorText(e)}`);
      return null;
    } finally {
      retrying.value = "";
      stage.value = "";
      done.value = 0;
      total.value = 0;
    }
  }

  async function remove(id: string) {
    try {
      await modpackApi.remove(id);
      await refresh();
      notify.success(`已删除实例 ${id}`);
    } catch (e) {
      notify.error(`删除失败：${errorText(e)}`);
    }
  }

  return {
    preview,
    zipPath,
    inspecting,
    importing,
    retrying,
    stage,
    done,
    total,
    percent,
    logs,
    instances,
    last,
    bind,
    pick,
    inspect,
    importPack,
    refresh,
    retry,
    remove,
  };
});