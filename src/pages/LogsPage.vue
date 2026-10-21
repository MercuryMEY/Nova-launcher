<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { useLaunchStore } from "../stores/launch";

const launcher = useLaunchStore();

/** null 表示看实时输出，否则看选中的历史日志 */
const picked = ref<string | null>(null);
const lines = ref<string[]>([]);
const truncated = ref(false);
const loading = ref(false);
const viewer = ref<HTMLElement | null>(null);

const isLive = computed(() => picked.value === null);
const shown = computed(() => (isLive.value ? launcher.live : lines.value));

function toBottom() {
  void nextTick(() => {
    const el = viewer.value;
    if (el) el.scrollTop = el.scrollHeight;
  });
}

watch(
  () => launcher.live.length,
  () => {
    if (isLive.value) toBottom();
  }
);

async function open(name: string) {
  picked.value = name;
  loading.value = true;
  truncated.value = false;
  try {
    const tail = await launcher.readLog(name, 1200);
    if (tail) {
      lines.value = tail.lines;
      truncated.value = tail.truncated;
    } else {
      lines.value = [];
    }
  } finally {
    loading.value = false;
    toBottom();
  }
}

function watchLive() {
  picked.value = null;
  lines.value = [];
  toBottom();
}

async function remove(name: string, ev: Event) {
  ev.stopPropagation();
  await launcher.deleteLog(name);
  if (picked.value === name) watchLive();
}

function sizeText(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

onMounted(() => {
  void launcher.loadLogs();
  void launcher.refresh();
  toBottom();
});
</script>

<template>
  <div class="logs">
    <aside class="side">
      <button class="live" :class="{ on: isLive }" @click="watchLive">
        <span class="dot" :class="{ run: launcher.running }"></span>
        实时输出
        <em v-if="launcher.live.length">{{ launcher.live.length }}</em>
      </button>

      <div class="head">
        <span>历史日志</span>
        <button class="ghost small" @click="launcher.loadLogs()">刷新</button>
      </div>

      <ul class="files">
        <li
          v-for="f in launcher.logFiles"
          :key="f.name"
          :class="{ on: picked === f.name }"
          @click="open(f.name)"
        >
          <div class="fname" :title="f.name">{{ f.name }}</div>
          <div class="meta">
            <span>{{ f.modified }}</span>
            <span>{{ sizeText(f.size) }}</span>
          </div>
          <button class="del" title="删除" @click="remove(f.name, $event)">×</button>
        </li>
      </ul>

      <div v-if="launcher.logFiles.length === 0" class="empty">还没有日志，启动一次游戏就有了</div>
    </aside>

    <section class="viewer-wrap">
      <div class="bar">
        <span class="title">{{ isLive ? "实时输出" : picked }}</span>
        <span v-if="launcher.running && isLive" class="badge">运行中 · PID {{ launcher.status?.pid }}</span>
        <span v-if="truncated" class="badge warn">只显示文件末尾</span>
        <button class="ghost small" @click="toBottom">拉到底部</button>
      </div>

      <div ref="viewer" class="viewer">
        <div v-if="loading" class="hint">正在读取…</div>
        <div v-else-if="shown.length === 0" class="hint">
          {{ isLive ? "启动游戏后，这里会实时滚动输出" : "这个日志是空的" }}
        </div>
        <pre v-else>
{{ shown.join("\n") }}</pre>
      </div>
    </section>
  </div>
</template>

<style scoped>
.logs {
  display: flex;
  height: 100%;
  min-height: 0;
}

.side {
  width: 260px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  padding: 14px 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
}

.live {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 12px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  color: var(--text-dim);
  font-size: 13px;
  text-align: left;
}

.live.on {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}

.live em {
  margin-left: auto;
  font-style: normal;
  font-size: 11px;
  color: var(--text-faint);
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--text-faint);
}

.dot.run {
  background: #22c55e;
  box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.2);
}

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-faint);
  padding: 0 4px;
  margin-top: 4px;
}

.files {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.files li {
  position: relative;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.files li:hover {
  background: var(--surface-hover);
}

.files li.on {
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}

.fname {
  font-size: 12px;
  font-family: var(--font-mono);
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta {
  display: flex;
  gap: 8px;
  font-size: 11px;
  color: var(--text-faint);
  margin-top: 3px;
}

.del {
  position: absolute;
  right: 6px;
  top: 6px;
  width: 18px;
  height: 18px;
  line-height: 1;
  border-radius: 4px;
  color: var(--text-faint);
  opacity: 0;
  transition: opacity 0.12s var(--ease);
}

.files li:hover .del {
  opacity: 1;
}

.del:hover {
  color: #ef4444;
  background: var(--surface-hover);
}

.empty {
  font-size: 12px;
  color: var(--text-faint);
  padding: 6px 4px;
  line-height: 1.6;
}

.viewer-wrap {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
}

.bar .title {
  font-size: 13px;
  font-family: var(--font-mono);
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.bar .ghost {
  margin-left: auto;
}

.badge {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  color: var(--accent);
}

.badge.warn {
  background: rgba(245, 158, 11, 0.16);
  color: #f59e0b;
}

.viewer {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 12px 14px;
}

.viewer pre {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.55;
  color: var(--text-dim);
  white-space: pre-wrap;
  word-break: break-all;
}

.hint {
  font-size: 12px;
  color: var(--text-faint);
}
</style>
