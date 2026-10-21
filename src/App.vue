<script setup lang="ts">
import { computed, onMounted, provide, ref } from "vue";
import TitleBar from "./components/TitleBar.vue";
import SideNav from "./components/SideNav.vue";
import StatusBar from "./components/StatusBar.vue";
import LaunchPage from "./pages/LaunchPage.vue";
import DownloadsPage from "./pages/DownloadsPage.vue";
import LogsPage from "./pages/LogsPage.vue";
import CrashPage from "./pages/CrashPage.vue";
import AccountPage from "./pages/AccountPage.vue";
import MultiplayerPage from "./pages/MultiplayerPage.vue";
import ModsPage from "./pages/ModsPage.vue";
import ModpackPage from "./pages/ModpackPage.vue";
import SettingsPage from "./pages/SettingsPage.vue";
import AboutPage from "./pages/AboutPage.vue";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsStore } from "./stores/settings";
import { useAccountStore } from "./stores/account";
import { useNotifyStore } from "./stores/notify";

const settings = useSettingsStore();
const account = useAccountStore();
const current = ref("launch");

// 页面间跳转（比如启动页点账户条去账户页），子页面用 inject 拿
provide("navigate", (key: string) => {
  current.value = key;
});

const views: Record<string, any> = {
  launch: LaunchPage,
  account: AccountPage,
  logs: LogsPage,
  crash: CrashPage,
  downloads: DownloadsPage,
  multiplayer: MultiplayerPage,
  mods: ModsPage,
  modpack: ModpackPage,
  settings: SettingsPage,
  about: AboutPage,
};

const view = computed(() => views[current.value] ?? LaunchPage);

const notify = useNotifyStore();

onMounted(async () => {
  await settings.load();
  await account.load();
  try {
    // 启动阶段的非致命错误（配置损坏、网络组件失败）以通知形式告知
    const err = await invoke<string | null>("get_init_error");
    if (err) notify.error(err);
  } catch {
    /* 非 Tauri 环境忽略 */
  }
});
</script>

<template>
  <div class="app">
    <TitleBar />
    <div class="body">
      <SideNav :current="current" @navigate="current = $event" />
      <main class="content">
        <Transition name="fade" mode="out-in">
          <component :is="view" :key="current" />
        </Transition>
      </main>
    </div>
    <StatusBar />
  </div>
</template>

<style scoped>
.app {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--bg);
}

.body {
  flex: 1;
  min-height: 0;
  display: flex;
}

.content {
  flex: 1;
  min-width: 0;
  overflow: hidden;
}
</style>
