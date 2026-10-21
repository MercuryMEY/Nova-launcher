<script setup lang="ts">
import { computed, inject, onMounted, ref, watch } from "vue";
import { useSettingsStore } from "../stores/settings";
import { useNotifyStore } from "../stores/notify";
import { useVersionStore, type VersionFilter } from "../stores/version";
import { useAccountStore } from "../stores/account";
import { useLaunchStore } from "../stores/launch";

const settings = useSettingsStore();
const versions = useVersionStore();
const notify = useNotifyStore();
const account = useAccountStore();
const launcher = useLaunchStore();
/** App.vue 里 provide 出来的页面跳转，没有就什么也不做 */
const navigate = inject<(key: string) => void>("navigate", () => {});
/** 只有「缺文件」类错误才给去修复入口：Java 不对时修版本没用 */
const needsRepair = computed(() => /依赖库|client\.jar|native/.test(launcher.lastError));
const avatarText = computed(() => (account.current?.name ?? "?").slice(0, 1).toUpperCase());
const avatarStyle = computed(() => {
  const n = Number.parseInt((account.current?.uuid ?? "888888").slice(0, 6), 16);
  const hue = Number.isNaN(n) ? 210 : n % 360;
  return { background: `hsl(${hue}, 55%, 42%)`, color: "#fff" };
});
const accountHint = computed(() => {
  const a = account.current;
  if (!a) return "点这里添加离线账户，不需要正版";
  return a.uuid_mode === "name-based" ? "离线账户 · UUID 按名字派生" : "离线账户 · 随机 UUID";
});

/** Java 提示优先用后端真实探测结果，拿不到的时候退回版本 JSON 的声明值 */
const javaTargetText = computed(() => {
  const st = launcher.javaStatus;
  if (st) return `Java ${st.target}`;
  const p = versions.preview?.java_major;
  return p ? `Java ${p}` : "未读取到";
});

const buttonText = computed(() => {
  if (!versions.preview) return "请先安装这个版本";
  if (launcher.running) return "游戏正在运行";
  if (launcher.launching) return "正在启动…";
  if (!launcher.javaStatus?.chosen) return "先准备 Java";
  return "启动游戏";
});

const showArgs = ref(false);

const filters: { value: VersionFilter; label: string }[] = [
  { value: "release", label: "正式版" },
  { value: "snapshot", label: "快照" },
  { value: "installed", label: "已装" },
  { value: "all", label: "全部" },
];

const sourceLabel = computed(() => {
  switch (versions.source) {
    case "official":
      return "官方源";
    case "mirror":
      return "BMCLAPI 镜像";
    case "cache":
      return "本地缓存";
    default:
      return "";
  }
});

function kindLabel(kind: string) {
  if (kind === "release") return "正式版";
  if (kind === "snapshot") return "快照";
  if (kind === "local") return "本地";
  return "旧版本";
}

function pick(id: string) {
  showArgs.value = false;
  void versions.select(id);
}

function refresh() {
  void versions.loadAll(true);
}

async function launch() {
  const id = versions.selectedId;
  if (!id) return;
  const summary = await launcher.launch(id);
  if (summary && summary.natives_missing > 0) {
    notify.error(`还缺 ${summary.natives_missing} 个 native 包，可能在初始化时崩掉`);
  }
}

async function stop() {
  await launcher.stop();
}

async function installJava() {
  const id = versions.selectedId;
  if (!id) return;
  const r = await launcher.installJava(id);
  if (r) notify.success("Java 就绪，可以启动了");
}

// 换版本就重新确认 Java 够不够，不等用户点启动才报错
watch(
  () => versions.selectedId,
  (id) => {
    if (id) void launcher.loadJava(id);
  }
);

onMounted(async () => {
  launcher.bind();
  await versions.loadAll();
  await launcher.refresh();
  if (versions.selectedId) void launcher.loadJava(versions.selectedId);
});
</script>

<template>
  <div class="launch">
    <section class="account-bar clickable" @click="navigate('account')">
      <div class="avatar" :style="avatarStyle">{{ avatarText }}</div>
      <div class="meta">
        <div class="name">{{ account.current?.name ?? "未登录" }}</div>
        <div class="hint">{{ accountHint }}</div>
      </div>
      <span v-if="sourceLabel" class="source">{{ sourceLabel }}</span>
      <button class="ghost small" @click.stop="navigate('account')">
        {{ account.current ? "切换" : "添加账户" }}
      </button>
    </section>

    <div class="main">
      <aside class="version-panel">
        <div class="panel-head">
          <span>版本列表</span>
          <button class="ghost small" :disabled="versions.loading" @click="refresh">
            {{ versions.loading ? "加载中" : "刷新" }}
          </button>
        </div>

        <div class="filters">
          <button
            v-for="f in filters"
            :key="f.value"
            class="filter"
            :class="{ active: versions.filter === f.value }"
            @click="versions.setFilter(f.value)"
          >
            {{ f.label }}
          </button>
        </div>

        <div v-if="versions.error" class="list-state error">
          <div>{{ versions.error }}</div>
          <button class="ghost small" @click="refresh">重试</button>
        </div>

        <div v-if="versions.loading && versions.list.length === 0" class="list-state">
          <div v-for="i in 6" :key="i" class="skeleton"></div>
        </div>

        <ul v-else class="version-list">
          <li
            v-for="v in versions.list"
            :key="v.id"
            :class="{ active: v.id === versions.selectedId }"
            @click="pick(v.id)"
          >
            <span class="v-id" :title="v.id">{{ v.id }}</span>
            <span class="v-kind">{{ kindLabel(v.kind) }}</span>
            <span v-if="v.installed" class="v-installed">已装</span>
          </li>
        </ul>

        <div v-if="!versions.loading && !versions.error && versions.list.length === 0" class="list-state">
          这个分类下没有版本
        </div>
      </aside>

      <section class="launch-panel">
        <div class="version-title">
          <h1>{{ versions.selectedId ?? "未选择版本" }}</h1>
          <span v-if="versions.selectedVersion" class="tag">
            {{ kindLabel(versions.selectedVersion.kind) }}
          </span>
          <span v-if="versions.selectedVersion?.installed" class="tag ok">已安装</span>
        </div>

        <div v-if="!versions.selectedId" class="empty-hint">
          从左边选一个版本，右侧会显示它的依赖、Java 要求和真实启动参数
        </div>

        <template v-else>
          <div class="summary">
            <div class="row">
              <span>内存</span>
              <b>{{ settings.settings.max_memory_mb }} MB</b>
            </div>
            <div class="row">
              <span>需要 Java</span>
              <b>
                {{ javaTargetText }}
              </b>
            </div>
            <div class="row">
              <span>游戏目录</span>
              <b :title="versions.preview?.game_dir">
                {{ versions.preview?.game_dir ?? "未安装" }}
              </b>
            </div>
            <div v-if="sourceLabel" class="row">
              <span>版本源</span>
              <b>{{ sourceLabel }}</b>
            </div>
          </div>

          <div v-if="versions.previewLoading" class="empty-hint">正在读取版本文件…</div>

          <template v-else-if="versions.preview">
            <div class="metrics">
              <div class="metric">
                <strong>{{ versions.preview.libraries }}</strong>
                <em>依赖库</em>
              </div>
              <div class="metric">
                <strong>{{ versions.preview.jvm_args.length }}</strong>
                <em>JVM 参数</em>
              </div>
              <div class="metric">
                <strong>{{ versions.preview.game_args.length }}</strong>
                <em>游戏参数</em>
              </div>
              <div class="metric" :class="{ warn: versions.preview.missing_files.length > 0 }">
                <strong>{{ versions.preview.missing_files.length }}</strong>
                <em>缺失文件</em>
              </div>
            </div>

            <div v-if="versions.preview.missing_files.length" class="warn-box">
              <div>
                还缺 {{ versions.preview.missing_files.length }} 个文件，下载引擎接入后会补齐
              </div>
              <code>{{ versions.preview.missing_files[0] }}</code>
            </div>

            <div v-if="!versions.preview.client_jar_exists" class="warn-box">
              <div>缺少该版本的 client.jar，无法进入游戏</div>
            </div>

            <button class="ghost small align-left" @click="showArgs = !showArgs">
              {{ showArgs ? "收起启动参数" : "查看启动参数" }}
            </button>

            <pre v-if="showArgs" class="args">{{ versions.preview.jvm_args.join("\n") }}</pre>
            <pre v-if="showArgs" class="args">{{ versions.preview.game_args.join("\n") }}</pre>
          </template>

          <div v-else class="empty-hint">
            本机还没有这个版本，需要先安装（下载引擎在下一批交付）
          </div>

          <div v-if="launcher.javaStatus" class="java-line" :class="{ warn: !launcher.javaStatus.chosen }">
            <template v-if="launcher.javaStatus.chosen">
              <span class="jl-label">用这份 Java {{ launcher.javaStatus.target }}</span>
              <code :title="launcher.javaStatus.chosen">{{ launcher.javaStatus.chosen }}</code>
            </template>
            <template v-else>
              <span class="jl-label">本机没有可用的 Java {{ launcher.javaStatus.target }}</span>
              <button class="ghost small" :disabled="launcher.installing" @click="installJava">
                {{ launcher.installing ? `下载中 ${launcher.installPercent}%` : "一键下载" }}
              </button>
            </template>
          </div>

          <div v-if="launcher.installing && launcher.installStage" class="stage">
            {{ launcher.installStage }}
            <span v-if="launcher.installPercent">· {{ launcher.installPercent }}%</span>
          </div>
          <div v-else-if="launcher.stage" class="stage">{{ launcher.stage }}</div>

          <button
            class="primary-btn"
            :disabled="!versions.preview || launcher.launching || launcher.running"
            @click="launch"
          >
            {{ buttonText }}
          </button>

          <div v-if="launcher.lastError" class="launch-error">
            <div class="le-text">{{ launcher.lastError }}</div>
            <button v-if="needsRepair" class="ghost small" @click="navigate('downloads')">
              去下载页修复
            </button>
          </div>

          <div class="run-row">
            <button v-if="launcher.running" class="ghost small" :disabled="launcher.stopping" @click="stop">
              {{ launcher.stopping ? "正在结束" : "停止游戏" }}
            </button>
            <button v-if="launcher.running || launcher.status" class="ghost small" @click="navigate('logs')">
              看日志
            </button>
          </div>

          <div class="footnote">
            {{
              launcher.running
                ? `运行中 · PID ${launcher.status?.pid ?? "-"} · ${launcher.status?.started_at ?? ""}`
                : "启动前会解压 natives、挑一份够用的 Java，游戏输出全部写进日志"
            }}
          </div>
        </template>
      </section>
    </div>
  </div>
</template>

<style scoped>
.launch {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 16px;
}

.account-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow);
}

.account-bar.clickable {
  cursor: pointer;
  transition: border-color 0.15s var(--ease);
}
.account-bar.clickable:hover {
  border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
}
.avatar {
  width: 38px;
  height: 38px;
  border-radius: var(--radius-sm);
  background: var(--surface-hover);
  display: grid;
  place-items: center;
  color: var(--text-faint);
  font-weight: 600;
}

.meta {
  flex: 1;
  min-width: 0;
}

.name {
  font-size: 14px;
  font-weight: 600;
}

.hint {
  font-size: 12px;
  color: var(--text-faint);
  margin-top: 2px;
}

.source {
  font-size: 12px;
  color: var(--text-dim);
  padding: 3px 9px;
  border-radius: 999px;
  border: 1px solid var(--border);
}

.main {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 280px 1fr;
  gap: 14px;
}

.version-panel {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px;
  border-bottom: 1px solid var(--border);
  font-size: 13px;
  font-weight: 600;
}

.filters {
  display: flex;
  gap: 6px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
}

.filter {
  flex: 1;
  padding: 4px 0;
  font-size: 12px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  transition: background 0.14s var(--ease), color 0.14s var(--ease);
}

.filter:hover {
  color: var(--text);
}

.filter.active {
  background: var(--surface-hover);
  color: var(--text);
  font-weight: 600;
}

.version-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.version-list li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  color: var(--text-dim);
  cursor: pointer;
  transition: background 0.12s var(--ease), color 0.12s var(--ease);
}

.version-list li:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.version-list li.active {
  background: var(--accent-soft, rgba(80, 140, 255, 0.16));
  color: var(--text);
}

.v-id {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.v-kind {
  font-size: 11px;
  color: var(--text-faint);
}

.v-installed {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 999px;
  border: 1px solid var(--border);
  color: var(--text-dim);
}

.list-state {
  padding: 14px;
  font-size: 12px;
  color: var(--text-faint);
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: flex-start;
}

.list-state.error {
  color: var(--danger, #e05a5a);
}

.skeleton {
  height: 28px;
  border-radius: var(--radius-sm);
  background: var(--surface-hover);
  opacity: 0.5;
}

.launch-panel {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow);
  padding: 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  overflow-y: auto;
}

.version-title {
  display: flex;
  align-items: center;
  gap: 10px;
}

.version-title h1 {
  font-size: 22px;
  font-weight: 700;
}

.tag {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--surface-hover);
  color: var(--text-dim);
}

.tag.ok {
  color: var(--accent);
  border: 1px solid var(--accent);
  background: transparent;
}

.empty-hint {
  font-size: 13px;
  color: var(--text-faint);
  line-height: 1.7;
}

.summary {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px 14px;
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  border: 1px solid var(--border);
}

.row {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  font-size: 13px;
  color: var(--text-dim);
}

.row b {
  color: var(--text);
  font-weight: 500;
  max-width: 62%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.metrics {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 8px;
}

.metric {
  padding: 10px 12px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.metric strong {
  font-size: 18px;
  font-weight: 600;
}

.metric em {
  font-style: normal;
  font-size: 11px;
  color: var(--text-faint);
}

.metric.warn strong {
  color: var(--warning, #e0a13a);
}

.warn-box {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  border: 1px solid var(--warning, #e0a13a);
  background: rgba(224, 161, 58, 0.08);
  font-size: 12px;
  color: var(--text-dim);
}

.warn-box code {
  font-size: 11px;
  color: var(--text-faint);
  word-break: break-all;
}

.args {
  margin: 0;
  padding: 12px;
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  font-size: 11px;
  line-height: 1.6;
  color: var(--text-dim);
  overflow-x: auto;
  white-space: pre-wrap;
  word-break: break-all;
}

.primary-btn {
  height: 48px;
  border-radius: var(--radius-md);
  background: var(--accent);
  color: #fff;
  font-size: 16px;
  font-weight: 600;
  transition: background 0.15s var(--ease), transform 0.08s var(--ease);
  margin-top: auto;
}

.primary-btn:hover:not(:disabled) {
  background: var(--accent-hover);
}

.primary-btn:active:not(:disabled) {
  transform: scale(0.985);
}

.primary-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.footnote {
  font-size: 11px;
  color: var(--text-faint);
  text-align: center;
}

.ghost {
  padding: 7px 14px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  color: var(--text-dim);
  font-size: 13px;
  transition: background 0.14s var(--ease), color 0.14s var(--ease);
}

.ghost:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.ghost.small {
  padding: 3px 10px;
  font-size: 12px;
}

.ghost.align-left {
  align-self: flex-start;
}

.java-line {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  font-size: 12px;
  color: var(--text-dim);
}

.java-line.warn {
  border-color: rgba(245, 158, 11, 0.5);
  color: #f59e0b;
}

.jl-label {
  flex-shrink: 0;
}

.java-line code {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  direction: rtl;
  text-align: left;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-faint);
}

.stage {
  font-size: 12px;
  color: var(--accent);
}

.run-row {
  display: flex;
  gap: 8px;
  margin-top: 8px;
}

.launch-error {
  margin-top: 8px;
  padding: 8px 10px;
  border: 1px solid #ef4444;
  border-radius: 8px;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.launch-error .le-text {
  flex: 1;
  color: #ef4444;
  word-break: break-all;
}
</style>
