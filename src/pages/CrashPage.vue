<script setup lang="ts">
import { computed, inject, onMounted } from "vue";
import { SEVERITY_LABELS, type Severity } from "../api/crash";
import { useCrashStore } from "../stores/crash";
import { useLaunchStore } from "../stores/launch";

const crash = useCrashStore();
const launcher = useLaunchStore();
const go = inject<(key: string) => void>("navigate", () => {});

/** 有退出码就带上：同一条日志，退出码能让结论确定不少 */
const code = computed(() => launcher.status?.exit?.code ?? null);

const SEV_CLASS: Record<Severity, string> = {
  fatal: "fatal",
  warn: "warn",
  info: "info",
};

function sizeText(n: number) {
  if (n <= 0) return "0 B";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

async function run(name: string | null) {
  await crash.analyze(name, code.value);
}

onMounted(async () => {
  if (!launcher.logFiles.length) await launcher.loadLogs();
  // 进来就顺手把最新那份分析了：崩完点开这一页，答案应该已经在眼前
  if (!crash.report && launcher.logFiles.length > 0) await run(null);
});
</script>

<template>
  <div class="crash">
    <aside class="side">
      <button class="latest" :disabled="crash.analyzing" @click="run(null)">
        {{ crash.analyzing ? "分析中…" : "分析最新日志" }}
      </button>
      <div class="head">
        <span>历史日志</span>
        <button class="ghost small" @click="launcher.loadLogs()">刷新</button>
      </div>
      <ul class="files">
        <li
          v-for="f in launcher.logFiles"
          :key="f.name"
          :class="{ on: crash.source === f.name }"
          @click="run(f.name)"
        >
          <div class="fname" :title="f.name">{{ f.name }}</div>
          <div class="meta">
            <span>{{ f.modified }}</span>
            <span>{{ sizeText(f.size) }}</span>
          </div>
        </li>
      </ul>
      <div v-if="!launcher.logFiles.length" class="empty">还没有日志，启动一次游戏就有了</div>
    </aside>

    <section class="main">
      <div class="bar">
        <span class="title">{{ crash.source || "最新日志" }}</span>
        <span v-if="code !== null" class="badge">退出码 {{ code }}</span>
        <span v-if="crash.report?.truncated" class="badge warn">只看了尾部</span>
        <span class="grow"></span>
        <button class="ghost small" @click="go('logs')">看原始日志</button>
      </div>

      <div v-if="!crash.report" class="card empty">
        <p>{{ crash.analyzing ? "正在扫日志…" : "选一份日志开始分析" }}</p>
        <p class="dim small">
          分析全在本地跑，不上传任何内容；规则只认已知的报错特征，没命中不代表没问题
        </p>
      </div>

      <template v-else>
        <div class="card glance" :class="SEV_CLASS[crash.report.severity]">
          <div class="row">
            <span class="sev">{{ SEVERITY_LABELS[crash.report.severity] }}</span>
            <h2>{{ crash.report.headline }}</h2>
          </div>
          <p class="dim small">
            扫了 {{ crash.report.scanned }} 行 · 命中 {{ crash.report.findings.length }} 条
          </p>
          <p v-if="crash.report.exit_hint" class="exit">{{ crash.report.exit_hint }}</p>
        </div>

        <article
          v-for="f in crash.report.findings"
          :key="f.id"
          class="finding"
          :class="SEV_CLASS[f.severity]"
        >
          <header>
            <span class="sev">{{ SEVERITY_LABELS[f.severity] }}</span>
            <h3>{{ f.title }}</h3>
          </header>
          <p class="detail">{{ f.detail }}</p>

          <div v-if="f.evidence.length" class="evidence">
            <div v-for="e in f.evidence" :key="e.line" class="evline">
              <span class="ln">{{ e.line }}</span>
              <code>{{ e.text }}</code>
            </div>
          </div>

          <ul v-if="f.advice.length" class="advice">
            <li v-for="(a, i) in f.advice" :key="i">{{ a }}</li>
          </ul>
        </article>

        <div v-if="!crash.report.findings.length" class="card">
          <h3>没有匹配到已知的报错</h3>
          <p class="dim small">
            这不等于没问题：卡死、被任务管理器强杀、崩在 native 层不留日志的情况都扫不出来。
            可以先去日志页看最后几十行，或者把日志贴给懂的人
          </p>
        </div>
      </template>
    </section>
  </div>
</template>

<style scoped>
.crash {
  display: flex;
  height: 100%;
  min-height: 0;
}

.side {
  width: 300px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  padding: 14px 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
}

.latest {
  padding: 9px 12px;
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  background: var(--accent);
  color: #fff;
  font-size: 13px;
  font-family: inherit;
  cursor: pointer;
}

.latest:disabled {
  opacity: 0.6;
  cursor: default;
}

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-faint);
}

.files {
  list-style: none;
  margin: 0;
  padding: 0;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.files li {
  padding: 7px 9px;
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  cursor: pointer;
  transition: background 0.15s var(--ease);
}

.files li:hover {
  background: var(--surface-hover);
}

.files li.on {
  background: var(--surface);
  border-color: var(--border);
}

.fname {
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta {
  display: flex;
  gap: 8px;
  font-size: 11px;
  color: var(--text-faint);
  margin-top: 2px;
}

.empty {
  font-size: 12px;
  color: var(--text-faint);
  text-align: center;
  padding: 16px 0;
}

.main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow-y: auto;
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.bar {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}

.title {
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.grow {
  flex: 1;
}

.badge {
  font-size: 11px;
  padding: 2px 7px;
  border-radius: 999px;
  border: 1px solid var(--border);
  color: var(--text-faint);
  font-family: var(--font-mono);
}

.card {
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--surface);
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.card p {
  margin: 0;
}

.glance .row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.glance h2 {
  margin: 0;
  font-size: 16px;
  line-height: 1.5;
}

.glance.fatal {
  border-left: 3px solid var(--accent);
}

.glance.warn {
  border-left: 3px solid var(--text-dim);
}

.glance.info {
  border-left: 3px solid var(--border);
}

.exit {
  font-size: 12px;
  color: var(--text-dim);
  padding: 7px 10px;
  background: var(--bg-elevated);
  border-radius: var(--radius-sm);
}

.finding {
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--surface);
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.finding.fatal {
  border-left: 3px solid var(--accent);
}

.finding header {
  display: flex;
  align-items: center;
  gap: 8px;
}

.finding h3 {
  margin: 0;
  font-size: 14px;
}

.sev {
  font-size: 11px;
  padding: 2px 7px;
  border-radius: 999px;
  border: 1px solid var(--border);
  color: var(--text-faint);
  flex-shrink: 0;
}

.fatal .sev {
  color: var(--accent);
  border-color: var(--accent);
}

.detail {
  margin: 0;
  font-size: 12px;
  color: var(--text-dim);
  line-height: 1.6;
}

.evidence {
  background: var(--bg);
  border-radius: var(--radius-sm);
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.evline {
  display: flex;
  gap: 8px;
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.6;
}

.ln {
  color: var(--text-faint);
  flex-shrink: 0;
  min-width: 32px;
  text-align: right;
}

.evline code {
  color: var(--text-dim);
  white-space: pre-wrap;
  word-break: break-all;
}

.advice {
  margin: 0;
  padding-left: 18px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.advice li {
  font-size: 12px;
  color: var(--text-dim);
  line-height: 1.6;
}

.small {
  font-size: 11px;
}

.dim {
  color: var(--text-faint);
}

.warn {
  color: var(--accent);
}

button.ghost {
  padding: 5px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text);
  font-size: 12px;
  font-family: inherit;
  cursor: pointer;
}

button.ghost:hover {
  background: var(--surface-hover);
}

button.small {
  padding: 3px 8px;
  font-size: 11px;
}
</style>