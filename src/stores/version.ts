import { defineStore } from "pinia";
import { computed, ref } from "vue";
import {
  errorText,
  versionApi,
  type InstalledVersion,
  type RemoteVersion,
  type RemoteVersions,
  type VersionPreview,
} from "../api/version";
import { useNotifyStore } from "./notify";

export type VersionFilter = "release" | "snapshot" | "all" | "installed";

export const useVersionStore = defineStore("version", () => {
  const remote = ref<RemoteVersions | null>(null);
  const installed = ref<InstalledVersion[]>([]);
  const loading = ref(false);
  const error = ref("");
  const filter = ref<VersionFilter>("release");
  const selectedId = ref<string | null>(null);
  const preview = ref<VersionPreview | null>(null);
  const previewLoading = ref(false);

  const installedIds = computed(() => new Set(installed.value.map((v) => v.id)));

  /** 远端清单 + 本地版本合并成一份列表，本地独有（自建版）放最前面 */
  const list = computed<RemoteVersion[]>(() => {
    const all = remote.value?.versions ?? [];
    const ids = installedIds.value;
    const merged = all.map((v) => ({ ...v, installed: v.installed || ids.has(v.id) }));
    const extra: RemoteVersion[] = installed.value
      .filter((v) => !all.some((r) => r.id === v.id))
      .map((v) => ({ id: v.id, kind: "local", release_time: "", installed: true }));
    const pool = [...extra, ...merged];

    if (filter.value === "all") return pool;
    if (filter.value === "installed") return pool.filter((v) => v.installed);
    return pool.filter((v) => v.kind === filter.value);
  });

  const selectedVersion = computed(
    () => list.value.find((v) => v.id === selectedId.value) ?? null
  );
  const source = computed(() => remote.value?.source ?? "");

  function setFilter(next: VersionFilter) {
    filter.value = next;
  }

  async function loadPreview(id: string) {
    preview.value = null;
    if (!installedIds.value.has(id)) return;
    previewLoading.value = true;
    try {
      preview.value = await versionApi.preview(id);
    } catch {
      preview.value = null;
    } finally {
      previewLoading.value = false;
    }
  }

  async function loadAll(notify = false) {
    if (loading.value) return;
    loading.value = true;
    error.value = "";
    try {
      const [r, i] = await Promise.all([versionApi.remote(), versionApi.installed()]);
      remote.value = r;
      installed.value = i;

      if (!selectedId.value) {
        const firstInstalled = i.find((v) => v.has_json);
        selectedId.value = firstInstalled?.id ?? r.latest_release ?? i[0]?.id ?? null;
        if (selectedId.value) void loadPreview(selectedId.value);
      }
    } catch (e) {
      error.value = errorText(e);
      if (notify) useNotifyStore().error(error.value);
    } finally {
      loading.value = false;
    }
  }

  async function select(id: string) {
    selectedId.value = id;
    await loadPreview(id);
  }

  return {
    remote,
    installed,
    loading,
    error,
    filter,
    selectedId,
    preview,
    previewLoading,
    list,
    selectedVersion,
    source,
    setFilter,
    loadAll,
    select,
  };
});
