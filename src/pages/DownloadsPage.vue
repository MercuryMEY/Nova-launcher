<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useVersionStore, type VersionFilter } from "../stores/version";
import { useInstallStore } from "../stores/install";
import { useLoaderStore } from "../stores/loader";

const versions = useVersionStore();
const install = useInstallStore();
const loaders = useLoaderStore();

const filters: { key: VersionFilter; label: string }[] = [
  { key: "release", label: "正式版" },
  { key: "snapshot", label: "快照" },
  { key: "installed", label: "已安装" },
  { key: "all", label: "全部" },
];

const sizeLabel = computed(() => sizeText(install.preview?.total_bytes ?? 0));
// 模板里不用非空断言：数值型字段统一在这里给默认值，vue-tsc 更安静
const clientCount = computed(() => (install.preview?.has_client ? 1 : 0));
const loggingCount = computed(() => (install.preview?.has_logging ? 1 : 0));
const libCount = computed(() => install.preview?.libraries ?? 0);
const assetCount = computed(() => install.preview?.assets ?? 0);
const fileCount = computed(() => install.preview?.total_files ?? 0);
const hasPlan = computed(() => install.preview !== null);

/** 三态决定按钮文案：完整只是校验，缺一部分是补齐，全缺才是安装 */
const beginLabel = computed(() => {
  const st = install.preview?.status;
  if (st === "complete") return "校验并修复";
  if (st === "partial") return "补齐缺失文件";
  return "开始安装";
});

/** 缺什么直接说出来，最多列 5 个，剩下的只给数量 */
const missingLabel = computed(() => {
  const p = install.preview;
  if (!p || p.status === "complete" || !p.missing_names.length) return "";
  const head = p.missing_names.slice(0, 5).join("、");
  return p.missing_count > 5 ? `${head} 等 ${p.missing_count} 个文件` : p.missing_names.join("、");
});

/** 加载器版本的 id 长这样：fabric-loader-0.16.5-1.21.4 / 1.20.1-forge-47.2.0 */
function isLoaderId(id: string): boolean {
  const lower = id.toLowerCase();
  return (
    lower.includes("fabric-loader") ||
    lower.includes("quilt-loader") ||
    lower.includes("-forge-") ||
    lower.includes("-neoforge-")
  );
}

/** 只有选中原版版本时才谈得上“装加载器” */
const isBase = computed(() => !!versions.selectedId && !isLoaderId(versions.selectedId));
/** 加载器是原版外面的一层，原版没装好，装了也跑不起来 */
const canInstallLoader = computed(() => isBase.value && !!install.preview?.has_client);
const logText = computed(() => loaders.logs.join("\n"));

function kindText(kind: string): string {
  if (kind === "release") return "正式版";
  if (kind === "snapshot") return "快照";
  if (kind === "local") return "本地";
  return kind;
}

/** 字节数说成人话，玩家不用自己算 1024² */
function sizeText(bytes: number): string {
  if (!bytes) return "大小未知";
  const units = ["B", "KB", "MB", "GB"];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i += 1;
  }
  return `${n >= 10 || i === 0 ? Math.round(n) : n.toFixed(1)} ${units[i]}`;
}

onMounted(() => {
  install.bind();
  loaders.bind();
  void loaders.loadKinds();
  void versions.loadAll(true).then(() => {
    if (versions.selectedId) void install.loadPreview(versions.selectedId);
  });
});

/** 选中即预览：拉计划但不落盘，点安装之前就知道要下多少 */
async function pick(id: string) {
  if (install.running) return;
  versions.selectedId = id;
  await install.loadPreview(id);
  // 加载器区域跟着选中的原版走，选到加载器版本本身时清空
  void loaders.switchGame(isLoaderId(id) ? "" : id);
}

function reload() {
  void versions.loadAll(true).then(() => {
    if (versions.selectedId) {
      void install.loadPreview(versions.selectedId);
      void loaders.loadInstalled(versions.selectedId);
    }
  });
}

async function begin() {
  const id = versions.selectedId;
  if (!id) return;
  const r = await install.start(id);
  // 装完本地版本变了，“已装”标记要重新拉一遍
  if (r) await versions.loadAll();
}

/** 换加载器类型要重拉版本列表，各家版本号体系完全不同 */
function pickKind(k: string) {
  const id = versions.selectedId;
  if (!id || loaders.installing) return;
  void loaders.loadVersions(id, k);
}

async function installLoader() {
  const id = versions.selectedId;
  if (!id) return;
  const r = await loaders.install(id);
  // 新装的加载器会多出一个版本目录，列表要跟着更新
  if (r) await versions.loadAll();
}
</script>

<template>
  <div class="downloads">
    <header class="bar">
      <div>
        <h1>下载中心</h1>
        <p>选一个版本安装，缺什么补什么，装过的文件不会重复下</p>
      </div>
      <button class="ghost" :disabled="versions.loading" @click="reload">
        {{ versions.loading ? "正在拉清单" : "刷新清单" }}
      </button>
    </header>

    <div class="split">
      <section class="list">
        <div class="filters">
          <button
            v-for="f in filters"
            :key="f.key"
            class="chip"
            :class="{ on: versions.filter === f.key }"
            @click="versions.setFilter(f.key)"
          >
            {{ f.label }}
          </button>
        </div>

        <div class="rows">
          <button
            v-for="v in versions.list"
            :key="v.id"
            class="row"
            :class="{ on: versions.selectedId === v.id }"
            @click="pick(v.id)"
          >
            <span class="vid">{{ v.id }}</span>
            <span v-if="v.installed" class="tag ok">已装</span>
            <span class="tag">{{ kindText(v.kind) }}</span>
          </button>
          <p v-if="!versions.list.length" class="empty">
            {{ versions.loading ? "正在加载" : "这个筛选下还没有版本" }}
          </p>
        </div>
      </section>

      <section class="panel">
        <template v-if="versions.selectedId">
          <div class="head">
            <h2>{{ versions.selectedId }}</h2>
            <span v-if="install.preview?.status === 'complete'" class="tag ok">已安装</span>
            <span v-else-if="install.preview?.status === 'partial'" class="tag warn">
              缺 {{ install.preview.missing_count }} 个文件
            </span>
          </div>

          <div v-if="install.running || install.stage" class="progress">
            <div class="line">
              <span>{{ install.stage || "正在准备" }}</span>
              <em v-if="install.total > 0">{{ install.done }} / {{ install.total }}</em>
            </div>
            <div class="track">
              <div class="fill" :style="{ width: install.percent + '%' }"></div>
            </div>
          </div>

          <div v-if="hasPlan" class="cards">
            <div class="card">
              <b>{{ clientCount }}</b>
              <span>客户端 jar</span>
            </div>
            <div class="card">
              <b>{{ libCount }}</b>
              <span>依赖库</span>
            </div>
            <div class="card">
              <b>{{ assetCount }}</b>
              <span>资源文件</span>
            </div>
            <div class="card">
              <b>{{ loggingCount }}</b>
              <span>日志配置</span>
            </div>
          </div>

          <p v-if="hasPlan" class="sum">
            共 {{ fileCount }} 个文件，约 {{ sizeLabel }}
          </p>
          <p v-if="missingLabel" class="sum">还缺 {{ missingLabel }}</p>
          <p v-if="!hasPlan && install.previewing" class="sum">正在算这份计划</p>

          <div class="actions">
            <button v-if="install.running" class="danger" :disabled="install.cancelling" @click="install.cancel()">
              {{ install.cancelling ? "正在停止" : "取消安装" }}
            </button>
            <button v-else class="primary" :disabled="!versions.selectedId" @click="begin">
              {{ beginLabel }}
            </button>
          </div>

          <p v-if="install.last" class="last">
            上次：新下 {{ install.last.downloaded }} 个，跳过 {{ install.last.skipped }} 个，
            {{ sizeText(install.last.bytes) }}，用了 {{ install.last.seconds }} 秒
          </p>

          <p class="hint">
            安装会补齐客户端、依赖库、资源与日志配置；已存在的文件按 SHA1 校验通过就跳过，
            失败的话再点一次就能接着下
          </p>

          <hr class="sep" />

          <section v-if="isBase" class="loaders">
            <div class="lhead">
              <h3>装加载器</h3>
              <span>给 {{ versions.selectedId }} 套一层 Mod 加载器，装完在启动页选它启动</span>
            </div>

            <p v-if="!canInstallLoader" class="hint">
              先把 {{ versions.selectedId }} 原版装好，再往上装加载器；加载器只是原版外面的一层
            </p>

            <template v-else>
              <div class="chips">
                <button
                  v-for="k in loaders.kinds"
                  :key="k.key"
                  class="chip"
                  :class="{ on: loaders.kind === k.key }"
                  :disabled="loaders.installing"
                  @click="pickKind(k.key)"
                >
                  {{ k.label }}
                </button>
              </div>

              <p v-if="loaders.currentKind" class="hint">{{ loaders.currentKind.note }}</p>

              <p v-if="loaders.currentKind?.needs_installer" class="warn">
                这一家要跑官方安装器，需要本机有 Java；它会自己再下一批库，可能要几分钟，
                期间别关窗口
              </p>

              <div class="pick">
                <select v-model="loaders.versionId" :disabled="loaders.installing || loaders.loadingVersions">
                  <option v-if="!loaders.versions.length" value="">
                    {{ loaders.loadingVersions ? "正在拉版本列表" : "这家没有可用版本" }}
                  </option>
                  <option v-for="v in loaders.versions" :key="v.id" :value="v.id">
                    {{ v.id }}{{ v.stable ? "（推荐）" : "" }}
                  </option>
                </select>
                <button
                  class="primary"
                  :disabled="loaders.installing || !loaders.versionId"
                  @click="installLoader"
                >
                  {{ loaders.installing ? "正在装" : "装加载器" }}
                </button>
              </div>

              <div v-if="loaders.stage || loaders.total > 0" class="progress">
                <div class="line">
                  <span>{{ loaders.stage || "正在准备" }}</span>
                  <em v-if="loaders.total > 0">{{ loaders.done }} / {{ loaders.total }}</em>
                </div>
                <div class="track">
                  <div class="fill" :style="{ width: loaders.percent + '%' }"></div>
                </div>
              </div>

              <pre v-if="logText" class="logs">{{ logText }}</pre>

              <p v-if="loaders.last" class="last">
                装好了：{{ loaders.last.id }}，新下 {{ loaders.last.downloaded }} 个，
                跳过 {{ loaders.last.skipped }} 个，用了 {{ loaders.last.seconds }} 秒
              </p>

              <div v-if="loaders.installed.length" class="installed">
                <span v-for="l in loaders.installed" :key="l.id" class="tag ok">
                  {{ l.id }} 已装
                </span>
              </div>
            </template>
          </section>

          <p v-else class="hint">
            这个版本本身就是加载器版本，想换加载器请在上面选中它的原版版本
          </p>
        </template>
        <p v-else class="empty">左边选一个版本</p>
      </section>
    </div>
  </div>
</template>

<style scoped>
.downloads {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.bar {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 16px;
  padding: 18px 20px 14px;
  border-bottom: 1px solid var(--border);
}

.bar h1 {
  margin: 0 0 4px;
  font-size: 18px;
}

.bar p {
  margin: 0;
  font-size: 12px;
  color: var(--text-dim);
}

.split {
  flex: 1;
  min-height: 0;
  display: flex;
}

.list {
  width: 280px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  padding: 12px 10px;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.filters {
  display: flex;
  gap: 6px;
  padding-bottom: 10px;
}

.chip {
  padding: 5px 10px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  color: var(--text-dim);
  font-size: 12px;
}

.chip.on {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}

.rows {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  background: transparent;
  color: var(--text);
  font-size: 13px;
  text-align: left;
}

.row:hover {
  background: var(--surface-hover);
}

.row.on {
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  border-color: color-mix(in srgb, var(--accent) 40%, transparent);
}

.vid {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tag {
  flex-shrink: 0;
  padding: 1px 6px;
  border-radius: 4px;
  border: 1px solid var(--border);
  color: var(--text-faint);
  font-size: 11px;
}

.tag.ok {
  border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  color: var(--accent);
}

.tag.warn {
  border-color: color-mix(in srgb, #f59e0b 45%, transparent);
  color: #f59e0b;
}

.panel {
  flex: 1;
  min-width: 0;
  padding: 18px 20px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.head {
  display: flex;
  align-items: center;
  gap: 10px;
}

.head h2 {
  margin: 0;
  font-size: 16px;
}

.progress {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.progress .line {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-dim);
}

.progress em {
  font-style: normal;
  color: var(--text-faint);
}

.track {
  height: 6px;
  border-radius: 999px;
  background: var(--surface);
  overflow: hidden;
}

.fill {
  height: 100%;
  background: var(--accent);
  transition: width 0.25s var(--ease);
}

.cards {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 10px;
}

.card {
  padding: 12px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.card b {
  font-size: 18px;
  font-variant-numeric: tabular-nums;
}

.card span {
  font-size: 12px;
  color: var(--text-dim);
}

.sum {
  margin: 0;
  font-size: 13px;
  color: var(--text-dim);
}

.actions {
  display: flex;
  gap: 10px;
}

.primary,
.danger,
.ghost {
  padding: 9px 16px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  color: var(--text);
}

.primary {
  border-color: transparent;
  background: var(--accent);
  color: #fff;
  font-weight: 600;
}

.danger {
  border-color: rgba(239, 68, 68, 0.5);
  color: #ef4444;
  background: transparent;
}

.primary:disabled,
.ghost:disabled,
.danger:disabled,
.chip:disabled {
  opacity: 0.55;
}

.last {
  margin: 0;
  font-size: 12px;
  color: var(--text-dim);
}

.hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-faint);
}

.sep {
  border: 0;
  border-top: 1px solid var(--border);
  margin: 2px 0;
}

.loaders {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.lhead {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.lhead h3 {
  margin: 0;
  font-size: 14px;
}

.lhead span {
  font-size: 12px;
  color: var(--text-dim);
}

.chips {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.warn {
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
  color: #f59e0b;
}

.pick {
  display: flex;
  gap: 10px;
  align-items: center;
}

.pick select {
  flex: 1;
  min-width: 0;
  padding: 9px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  color: var(--text);
  font-size: 13px;
}

.logs {
  margin: 0;
  max-height: 160px;
  overflow-y: auto;
  padding: 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-dim);
  font-size: 11px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
}

.installed {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.empty {
  margin: 0;
  padding: 12px 4px;
  font-size: 13px;
  color: var(--text-faint);
}
</style>