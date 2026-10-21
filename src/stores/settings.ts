import { defineStore } from "pinia";
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useNotifyStore } from "./notify";

export type IsolationMode = "full" | "none" | "non-fake";
/** 游戏启动时的全屏策略。ignore = 不动 options.txt */
export type FullscreenMode = "ignore" | "on" | "off";

export interface Settings {
  max_memory_mb: number;
  java_path: string | null;
  /** 游戏分辨率（拼进 --width / --height） */
  window_width: number;
  window_height: number;
  /** 旧字段，仅用于后端迁移，界面不再展示 */
  fullscreen: boolean;
  fullscreen_mode: FullscreenMode;
  /** 启动器窗口自身的尺寸，与游戏分辨率无关 */
  launcher_width: number;
  launcher_height: number;
  launcher_maximized: boolean;
  isolation: IsolationMode;
  theme: "light" | "dark" | "system";
  accent: string;
  mirror_timeout_secs: number;
  prefer_mirror: boolean;
}

const ACCENTS = ["blue", "purple", "green", "orange", "pink", "red", "cyan", "amber"];

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<Settings>({
    max_memory_mb: 2048,
    java_path: null,
    window_width: 1280,
    window_height: 720,
    fullscreen: false,
    fullscreen_mode: "ignore",
    launcher_width: 1180,
    launcher_height: 760,
    launcher_maximized: false,
    isolation: "non-fake",
    theme: "dark",
    accent: "blue",
    mirror_timeout_secs: 4,
    prefer_mirror: false,
  });
  const loaded = ref(false);
  const saving = ref(false);

  function applyTheme() {
    const root = document.documentElement;
    const preferDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;
    const dark = settings.value.theme === "system" ? preferDark : settings.value.theme === "dark";
    root.dataset.theme = dark ? "dark" : "light";
    root.dataset.accent = ACCENTS.includes(settings.value.accent) ? settings.value.accent : "blue";
  }

  async function load() {
    try {
      const cfg: any = await invoke("get_config");
      if (cfg?.settings) settings.value = { ...settings.value, ...cfg.settings };
    } catch (e: any) {
      // 首次启动或非 Tauri 环境下静默用默认值，但首次使用要给一次可见提示
      if (loaded.value) useNotifyStore().error(e?.message ?? "读取设置失败");
    }
    loaded.value = true;
    applyTheme();
  }

  let lastError = "";
  async function save() {
    if (saving.value) return;
    saving.value = true;
    try {
      await invoke("save_settings", { settings: settings.value });
      lastError = "";
    } catch (e: any) {
      const msg = e?.message ?? "设置保存失败，请检查磁盘权限";
      // 同样文案只提示一次，避免拖动滑块时刷屏
      if (msg !== lastError) {
        lastError = msg;
        useNotifyStore().error(msg);
      }
    } finally {
      saving.value = false;
    }
  }

  let timer: number | undefined;
  watch(
    settings,
    () => {
      applyTheme();
      window.clearTimeout(timer);
      timer = window.setTimeout(() => save(), 400);
    },
    { deep: true }
  );

  return { settings, loaded, saving, load, save, accents: ACCENTS };
});
