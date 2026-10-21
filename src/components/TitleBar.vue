<script setup lang="ts">
import { onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";

const win = getCurrentWindow();
const isMac = ref(navigator.userAgent.includes("Mac"));
const maximized = ref(false);

async function syncMaximized() {
  try {
    maximized.value = await win.isMaximized();
  } catch {
    /* 非 Tauri 环境忽略 */
  }
}

onMounted(syncMaximized);

const minimize = () => win.minimize();
const close = () => win.close();
async function toggleMaximize() {
  await win.toggleMaximize();
  syncMaximized();
}
</script>

<template>
  <header class="titlebar" data-tauri-drag-region>
    <div v-if="isMac" class="controls mac">
      <button class="dot close" title="关闭" @click="close" />
      <button class="dot min" title="最小化" @click="minimize" />
      <button class="dot max" title="最大化" @click="toggleMaximize" />
    </div>

    <div class="brand" data-tauri-drag-region>
      <span class="logo">▣</span>
      <span class="name">Nova Launcher</span>
      <span class="ver">0.1.0</span>
    </div>

    <div v-if="!isMac" class="controls win">
      <button class="ctl" title="最小化" @click="minimize">－</button>
      <button class="ctl" :title="maximized ? '还原' : '最大化'" @click="toggleMaximize">
        {{ maximized ? "❐" : "□" }}
      </button>
      <button class="ctl danger" title="关闭" @click="close">✕</button>
    </div>
  </header>
</template>

<style scoped>
.titlebar {
  height: var(--titlebar-height);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px;
  background: var(--bg-elevated);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.brand {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-left: 6px;
  font-size: 13px;
  pointer-events: none;
}

.logo {
  color: var(--accent);
  font-size: 15px;
}

.name {
  font-weight: 600;
  letter-spacing: 0.2px;
}

.ver {
  color: var(--text-faint);
  font-size: 11px;
}

.controls {
  display: flex;
  gap: 2px;
}

.ctl {
  width: 38px;
  height: 28px;
  border-radius: 6px;
  color: var(--text-dim);
  font-size: 12px;
  transition: background 0.14s var(--ease), color 0.14s var(--ease);
}

.ctl:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.ctl.danger:hover {
  background: #e5484d;
  color: #fff;
}

.controls.mac {
  gap: 8px;
  padding-left: 6px;
}

.dot {
  width: 12px;
  height: 12px;
  border-radius: 50%;
}

.dot.close { background: #ff5f57; }
.dot.min   { background: #febc2e; }
.dot.max   { background: #28c840; }

.dot:hover {
  filter: brightness(1.12);
}
</style>
