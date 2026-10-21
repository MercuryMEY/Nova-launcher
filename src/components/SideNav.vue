<script setup lang="ts">
defineProps<{ current: string }>();
const emit = defineEmits<{ (e: "navigate", key: string): void }>();

const items = [
  { key: "launch", label: "启动", icon: "M8 5v14l11-7z" },
  {
    key: "account",
    label: "账户",
    icon: "M12 12a4 4 0 100-8 4 4 0 000 8zM4 21c0-3.3 3.6-6 8-6s8 2.7 8 6",
  },
  { key: "logs", label: "日志", icon: "M8 6h9M8 10h9M8 14h6M5 4h14v16H5z" },
  { key: "crash", label: "崩溃分析", icon: "M12 8v5m0 3h.01M10.3 3.9L2.6 17a2 2 0 001.7 3h15.4a2 2 0 001.7-3L13.7 3.9a2 2 0 00-3.4 0z" },
  { key: "downloads", label: "下载", icon: "M12 3v12m0 0l-4-4m4 4l4-4M4 19h16" },
  { key: "mods", label: "Mod", icon: "M4 5h16v14H4zM12 9v6M9 12h6" },
  { key: "modpack", label: "整合包", icon: "M4 8l8-4 8 4v8l-8 4-8-4zM4 8l8 4 8-4M12 12v8" },
  { key: "multiplayer", label: "联机", icon: "M9 7h6M9 12h6M9 17h4M5 3h14v18H5z" },
  { key: "settings", label: "设置", icon: "M12 15a3 3 0 100-6 3 3 0 000 6zM4 12h2m12 0h2M12 4v2m0 12v2" },
  { key: "about", label: "其他", icon: "M12 8h.01M11 12h1v5h1M12 21a9 9 0 100-18 9 9 0 000 18z" },
];
</script>

<template>
  <nav class="sidenav">
    <button
      v-for="it in items"
      :key="it.key"
      class="item"
      :class="{ active: current === it.key }"
      @click="emit('navigate', it.key)"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7">
        <path :d="it.icon" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <span>{{ it.label }}</span>
    </button>
  </nav>
</template>

<style scoped>
.sidenav {
  width: var(--nav-width);
  flex-shrink: 0;
  padding: 12px 10px;
  background: var(--bg-elevated);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-radius: var(--radius-sm);
  color: var(--text-dim);
  font-size: 14px;
  text-align: left;
  transition: background 0.15s var(--ease), color 0.15s var(--ease);
}

.item:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.item.active {
  background: color-mix(in srgb, var(--accent) 16%, transparent);
  color: var(--accent);
  font-weight: 600;
}

.item svg {
  width: 18px;
  height: 18px;
  flex-shrink: 0;
}
</style>
