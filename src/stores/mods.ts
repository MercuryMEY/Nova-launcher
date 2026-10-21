import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  baseGameOf,
  detectLoader,
  errorText,
  modsApi,
  type LocalMod,
  type ModHit,
  type ModVersion,
  type SortKey,
} from "../api/mods";
import { useNotifyStore } from "./notify";

/** Modrinth 一次给 20 条，刚好一屏半，再多渲染就卡了 */
const PAGE = 20;

/** 一个整合包实例能给 Mod 页的信息，字段从 instance.json 来 */
export interface ModTarget {
  id: string;
  game: string;
  loader: string | null;
}

export const useModsStore = defineStore("mods", () => {
  const notify = useNotifyStore();

  /** 目标版本：Mod 就装进这个版本目录的 mods/，也决定搜索用的 MC 版本 */
  const target = ref("");
  /** 整合包实例：非空时 mods 目录改成 instances/<id>/mods */
  const instanceId = ref("");
  /** 实例自己声明的 MC 版本与加载器：实例目录名里没有版本号，只能靠它 */
  const instanceGame = ref("");
  const instanceLoader = ref("");

  /** 加载器筛选，默认从目标推出来，用户可改 */
  const loader = ref("");
  const query = ref("");
  const sort = ref<SortKey>("relevance");

  const hits = ref<ModHit[]>([]);
  const total = ref(0);
  const loading = ref(false);
  /** 搜过一次之后才显示「没有结果」，否则空列表会一直被当成搜过 */
  const searched = ref(false);

  /** 展开的项目详情 */
  const project = ref<ModHit | null>(null);
  const versions = ref<ModVersion[]>([]);
  const loadingVersions = ref(false);

  const localMods = ref<LocalMod[]>([]);
  const dir = ref("");
  const loadingLocal = ref(false);
  /** 正在下载的文件名，用来只让点中的那个按钮转圈 */
  const installing = ref("");
  /** 启停 / 删除时的占位，防连点 */
  const busyFile = ref("");

  /** 搜索用的 MC 版本：实例模式下用实例声明的那个 */
  const game = computed(() => (instanceId.value ? instanceGame.value : baseGameOf(target.value)));
  /** 界面判断「选没选目标」只看这个 */
  const canSearch = computed(() => !!game.value);
  /**
   * 传给后端做「拼路径用」的版本名。
   *
   * 版本模式下后端要拿它拼 `versions/<id>/mods`，所以必须是目录名本身；
   * 实例模式下后端只认 instance，这里给空串就行
   */
  const pathArg = computed(() => (instanceId.value ? "" : target.value));
  const installedNames = computed(() => new Set(localMods.value.map((m) => m.file_name)));
  const canLoadMore = computed(() => hits.value.length < total.value);

  /** 清掉搜索结果与详情，切目标时用 */
  function resetSearch() {
    project.value = null;
    versions.value = [];
    hits.value = [];
    total.value = 0;
    searched.value = false;
  }

  /** 切目标版本：搜索状态全清，本地清单重读 */
  function setTarget(id: string) {
    target.value = id;
    instanceId.value = "";
    instanceGame.value = "";
    instanceLoader.value = "";
    loader.value = detectLoader(id);
    resetSearch();
    void loadLocal();
  }

  /** 切到一个整合包实例：目录和搜索条件都跟着实例走 */
  function setInstance(inst: ModTarget | null) {
    instanceId.value = inst?.id ?? "";
    instanceGame.value = inst?.game ?? "";
    instanceLoader.value = inst?.loader ?? "";
    loader.value = instanceLoader.value;
    resetSearch();
    void loadLocal();
  }

  async function search(more = false) {
    if (!canSearch.value) {
      notify.info("先选一个目标版本或整合包实例");
      return;
    }
    if (loading.value) return;
    loading.value = true;
    const offset = more ? hits.value.length : 0;
    try {
      const page = await modsApi.search(
        game.value,
        loader.value,
        query.value,
        sort.value,
        offset,
        PAGE
      );
      hits.value = more ? [...hits.value, ...page.hits] : page.hits;
      total.value = page.total;
      searched.value = true;
    } catch (e) {
      if (!more) {
        hits.value = [];
        total.value = 0;
        searched.value = true;
      }
      notify.error(errorText(e));
    } finally {
      loading.value = false;
    }
  }

  async function openProject(hit: ModHit) {
    project.value = hit;
    versions.value = [];
    loadingVersions.value = true;
    try {
      versions.value = await modsApi.versions(hit.project_id, game.value, loader.value);
    } catch (e) {
      // 「这个 Mod 没有支持当前版本的版本」是很常见的正常情况，不必弹红条吓人
      versions.value = [];
      notify.info(errorText(e));
    } finally {
      loadingVersions.value = false;
    }
  }

  function closeProject() {
    project.value = null;
    versions.value = [];
  }

  async function installVersion(v: ModVersion) {
    if (!canSearch.value || installing.value) return;
    installing.value = v.file.filename;
    try {
      const r = await modsApi.install(game.value, instanceId.value || null, v.file);
      notify.success(r.skipped ? "这个文件已经在 mods 里了" : `装好了：${v.file.filename}`);
      await loadLocal();
    } catch (e) {
      notify.error(errorText(e));
    } finally {
      installing.value = "";
    }
  }

  async function loadLocal() {
    if (!canSearch.value) {
      localMods.value = [];
      dir.value = "";
      return;
    }
    const inst = instanceId.value || null;
    loadingLocal.value = true;
    try {
      const [list, d] = await Promise.all([
        modsApi.list(pathArg.value, inst),
        modsApi.dir(pathArg.value, inst),
      ]);
      localMods.value = list;
      dir.value = d;
    } catch {
      // 目录都还不存在是正常的（还没装第一个 Mod），读不到就当空
      localMods.value = [];
      dir.value = "";
    } finally {
      loadingLocal.value = false;
    }
  }

  async function toggle(mod: LocalMod) {
    if (busyFile.value) return;
    busyFile.value = mod.file_name;
    try {
      await modsApi.toggle(pathArg.value, instanceId.value || null, mod.file_name, !mod.enabled);
      await loadLocal();
    } catch (e) {
      notify.error(errorText(e));
    } finally {
      busyFile.value = "";
    }
  }

  async function remove(mod: LocalMod) {
    if (busyFile.value) return;
    busyFile.value = mod.file_name;
    try {
      await modsApi.remove(pathArg.value, instanceId.value || null, mod.file_name);
      notify.success(`删掉了 ${mod.name ?? mod.file_name}`);
      await loadLocal();
    } catch (e) {
      notify.error(errorText(e));
    } finally {
      busyFile.value = "";
    }
  }

  return {
    target,
    instanceId,
    instanceGame,
    instanceLoader,
    loader,
    query,
    sort,
    hits,
    total,
    loading,
    searched,
    project,
    versions,
    loadingVersions,
    localMods,
    dir,
    loadingLocal,
    installing,
    busyFile,
    game,
    canSearch,
    installedNames,
    canLoadMore,
    setTarget,
    setInstance,
    search,
    openProject,
    closeProject,
    installVersion,
    loadLocal,
    toggle,
    remove,
  };
});