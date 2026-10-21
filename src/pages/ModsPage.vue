<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import {
  LOADER_LABELS,
  detectLoader,
  type LocalMod,
  type ModVersion,
  type SortKey,
} from "../api/mods";
import { useModsStore } from "../stores/mods";
import { useVersionStore } from "../stores/version";
import { modpackApi, type ModpackInstance } from "../api/modpack";

const mods = useModsStore();
const versions = useVersionStore();

/** 目标有两类：普通游戏版本，或一个整合包实例 */
const mode = ref<"version" | "instance">("version");
const instances = ref<ModpackInstance[]>([]);
const loadingInstances = ref(false);

/** 只有装了 json 的版本才是真能启动的，其余的是装到一半的残骸 */
const installedVersions = computed(() => versions.installed.filter((v) => v.has_json));

const loaderLabel = computed(() => (mods.loader ? LOADER_LABELS[mods.loader] ?? mods.loader : "不限"));

const sortOptions = [
  { key: "relevance", label: "相关度" },
  { key: "downloads", label: "下载量" },
  { key: "follows", label: "关注数" },
  { key: "newest", label: "最新发布" },
  { key: "updated", label: "最近更新" },
];

/**
 * 删除做二次确认：点一下变成「确认删除」，3 秒没点第二下就退回。
 * 不用 window.confirm —— 各家 webview 的样式和返回值行为不一致，容易点完没反应。
 */
const armed = ref("");
let armTimer = 0;

function askRemove(m: LocalMod) {
  if (armed.value === m.file_name) {
    window.clearTimeout(armTimer);
    armed.value = "";
    void mods.remove(m);
    return;
  }
  armed.value = m.file_name;
  window.clearTimeout(armTimer);
  armTimer = window.setTimeout(() => {
    armed.value = "";
  }, 3000);
}

/** select 的 value 是裸 string，这里显式收窄回 SortKey 再触发搜索 */
function onSort(ev: Event) {
  mods.sort = (ev.target as HTMLSelectElement).value as SortKey;
  void mods.search();
}

/** 改加载器筛选：直接从默认值换掉，然后按新条件重搜一遍 */
function onLoader(ev: Event) {
  mods.loader = (ev.target as HTMLSelectElement).value;
  void mods.search();
}

function onTarget(ev: Event) {
  const el = ev.target as HTMLSelectElement;
  mods.setTarget(el.value);
}

function sizeText(n: number) {
  if (n <= 0) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function countText(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

function dateText(s: string) {
  return s ? s.slice(0, 10) : "";
}

/** 只数 required：optional / embedded 装了也不影响运行 */
function requiredDeps(v: ModVersion) {
  return v.dependencies.filter((d) => d.dependency_type === "required").length;
}

function installedNameOf(v: ModVersion) {
  return mods.installedNames.has(v.file.filename);
}

async function loadInstances() {
  loadingInstances.value = true;
  try {
    instances.value = await modpackApi.list();
  } catch {
    // 没有实例、或者 instances/ 还没建，都当空处理
    instances.value = [];
  } finally {
    loadingInstances.value = false;
  }
}

/** 选中一个实例：目录切到 instances/<id>/mods，搜索条件换成实例的版本 */
function pickInstance(id: string) {
  const ins = instances.value.find((i) => i.id === id);
  mods.setInstance(ins ? { id: ins.id, game: ins.game, loader: ins.loader } : null);
}

function onMode(m: "version" | "instance") {
  if (mode.value === m) return;
  mode.value = m;
  if (m === "instance") {
    const first = instances.value.find((i) => i.id === mods.instanceId) ?? instances.value[0];
    if (first) pickInstance(first.id);
    return;
  }
  // 切回版本：顺手把实例目标清掉，不然目录还指着 instances/
  mods.setTarget(mods.target || installedVersions.value[0]?.id || "");
}

function onInstance(ev: Event) {
  const id = (ev.target as HTMLSelectElement).value;
  if (!id) {
    mods.setInstance(null);
    return;
  }
  pickInstance(id);
}

onMounted(async () => {
  if (!versions.installed.length) await versions.loadAll();
  void loadInstances();
  if (mods.target && installedVersions.value.some((v) => v.id === mods.target)) {
    void mods.loadLocal();
    return;
  }
  // 默认挑一个装了加载器的版本：没加载器的版本装 Mod 等于白装
  const withLoader = installedVersions.value.find((v) => detectLoader(v.id));
  const first = withLoader ?? installedVersions.value[0];
  if (first) mods.setTarget(first.id);
});

onUnmounted(() => window.clearTimeout(armTimer));
</script>

<template>
  <div class="mods">
    <aside class="side">
      <div class="pick">
        <div class="picks">
          <button
            class="ghost small"
            :class="{ on: mode === 'version' }"
            @click="onMode('version')"
          >
            游戏版本
          </button>
          <button
            class="ghost small"
            :class="{ on: mode === 'instance' }"
            @click="onMode('instance')"
          >
            整合包实例
          </button>
        </div>

        <template v-if="mode === 'version'">
          <label>目标版本</label>
          <select :value="mods.target" @change="onTarget">
            <option value="">选一个已装版本</option>
            <option v-for="v in installedVersions" :key="v.id" :value="v.id">{{ v.id }}</option>
          </select>
        </template>

        <template v-else>
          <label>整合包实例</label>
          <select :value="mods.instanceId" :disabled="!instances.length" @change="onInstance">
            <option value="">选一个已导入实例</option>
            <option v-for="i in instances" :key="i.id" :value="i.id">
              {{ i.name }}（{{ i.game }}）
            </option>
          </select>
          <div v-if="!instances.length && !loadingInstances" class="empty small">
            还没有整合包实例，去整合包页导入一个
          </div>
        </template>
      </div>

      <div v-if="mods.target" class="tags">
        <span class="badge">{{ mods.game }}</span>
        <select class="loader" :value="mods.loader" @change="onLoader">
          <option value="">不限加载器</option>
          <option v-for="(label, key) in LOADER_LABELS" :key="key" :value="key">{{ label }}</option>
        </select>
      </div>

      <div class="head">
        <span>本地 Mod {{ mods.localMods.length ? `(${mods.localMods.length})` : "" }}</span>
        <button class="ghost small" :disabled="!mods.canSearch || mods.loadingLocal" @click="mods.loadLocal()">
          刷新
        </button>
      </div>

      <ul class="locals">
        <li v-for="m in mods.localMods" :key="m.file_name" :class="{ off: !m.enabled }">
          <div class="lname" :title="m.file_name">{{ m.name ?? m.file_name }}</div>
          <div class="lmeta">
            <span v-if="m.version">{{ m.version }}</span>
            <span v-if="m.loader">{{ LOADER_LABELS[m.loader] ?? m.loader }}</span>
            <span>{{ sizeText(m.size) }}</span>
          </div>
          <div class="lacts">
            <button
              class="ghost tiny"
              :disabled="mods.busyFile === m.file_name"
              @click="mods.toggle(m)"
            >
              {{ m.enabled ? "禁用" : "启用" }}
            </button>
            <button
              class="ghost tiny"
              :class="{ danger: armed === m.file_name }"
              :disabled="mods.busyFile === m.file_name"
              @click="askRemove(m)"
            >
              {{ armed === m.file_name ? "确认删除" : "删除" }}
            </button>
          </div>
        </li>
      </ul>

      <div v-if="!mods.canSearch" class="empty">先选一个版本或整合包实例，Mod 会装进它的 mods 目录</div>
      <div v-else-if="!mods.localMods.length && !mods.loadingLocal" class="empty">
        这里还没有 Mod，右边搜一个装上试试
      </div>

      <div v-if="mods.dir" class="dir" :title="mods.dir">{{ mods.dir }}</div>
    </aside>

    <section class="main">
      <template v-if="!mods.project">
        <div class="bar">
          <input
            v-model="mods.query"
            class="searchbox"
            placeholder="搜 Mod 名字，留空就是浏览热门"
            :disabled="!mods.canSearch"
            @keyup.enter="mods.search()"
          />
          <select :value="mods.sort" :disabled="!mods.canSearch" @change="onSort">
            <option v-for="s in sortOptions" :key="s.key" :value="s.key">{{ s.label }}</option>
          </select>
          <button class="primary" :disabled="!mods.canSearch || mods.loading" @click="mods.search()">
            {{ mods.loading ? "搜索中…" : "搜索" }}
          </button>
        </div>

        <div v-if="!mods.canSearch" class="hint big">选一个目标版本后就能搜 Mod 了</div>

        <div v-else class="list">
          <article
            v-for="hit in mods.hits"
            :key="hit.project_id"
            class="card"
            @click="mods.openProject(hit)"
          >
            <img v-if="hit.icon_url" class="icon" :src="hit.icon_url" alt="" loading="lazy" />
            <div v-else class="icon blank">{{ hit.title.slice(0, 1) }}</div>
            <div class="cbody">
              <div class="row">
                <h3>{{ hit.title }}</h3>
                <span class="author">{{ hit.author }}</span>
              </div>
              <p>{{ hit.description }}</p>
              <div class="meta">
                <span>↓ {{ countText(hit.downloads) }}</span>
                <span v-for="c in hit.categories.slice(0, 3)" :key="c" class="tagc">{{ c }}</span>
              </div>
            </div>
          </article>

          <div v-if="mods.loading && !mods.hits.length" class="hint big">正在问 Modrinth…</div>
          <div v-else-if="mods.searched && !mods.hits.length" class="hint big">
            没搜到东西，换个关键词，或者把加载器筛选改成「不限」
          </div>

          <div v-if="mods.canLoadMore && mods.hits.length" class="more">
            <button class="ghost" :disabled="mods.loading" @click="mods.search(true)">
              {{ mods.loading ? "加载中…" : `加载更多（还有 ${mods.total - mods.hits.length} 个）` }}
            </button>
          </div>
        </div>
      </template>

      <template v-else>
        <div class="bar">
          <button class="ghost" @click="mods.closeProject()">← 返回</button>
          <span class="title">{{ mods.project.title }}</span>
          <span class="author">{{ mods.project.author }}</span>
        </div>

        <div class="detail">
          <img v-if="mods.project.icon_url" class="bigicon" :src="mods.project.icon_url" alt="" />
          <p class="desc">{{ mods.project.description }}</p>
          <p class="hint">
            只列出支持 {{ mods.game }} + {{ loaderLabel }} 的文件；前置要自己另外装，这里只提示缺几个
          </p>

          <div v-if="mods.loadingVersions" class="hint big">正在找可用版本…</div>
          <div v-else-if="!mods.versions.length" class="hint big">
            这个 Mod 没有适配当前组合的版本，换个加载器筛选再看
          </div>

          <ul class="versions">
            <li v-for="v in mods.versions" :key="v.id">
              <div class="vtop">
                <strong :title="v.version_number">{{ v.name || v.version_number }}</strong>
                <span class="badge" :class="v.version_type">{{ v.version_type }}</span>
              </div>
              <div class="vmeta">
                <span class="mono">{{ v.file.filename }}</span>
                <span>{{ sizeText(v.file.size) }}</span>
                <span>{{ dateText(v.date_published) }}</span>
              </div>
              <p v-if="requiredDeps(v)" class="warn">
                这个文件声明了 {{ requiredDeps(v) }} 个必需前置，装之前先去把前置也装上
              </p>
              <div class="vacts">
                <span v-if="installedNameOf(v)" class="badge plain">已在 mods 里</span>
                <button
                  class="primary"
                  :disabled="!!mods.installing"
                  @click="mods.installVersion(v)"
                >
                  {{ mods.installing === v.file.filename ? "下载中…" : installedNameOf(v) ? "重新安装" : "安装" }}
                </button>
              </div>
            </li>
          </ul>
        </div>
      </template>
    </section>
  </div>
</template>

<style scoped>
.mods {
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

.pick {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.pick label {
  font-size: 12px;
  color: var(--text-faint);
}

select,
.searchbox {
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  color: var(--text);
  font-size: 13px;
  font-family: inherit;
}

select:disabled,
.searchbox:disabled {
  opacity: 0.6;
}

.tags select.loader {
  padding: 2px 6px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-dim);
  font-size: 11px;
}

.tags {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.badge {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  color: var(--accent);
  white-space: nowrap;
}

.badge.plain {
  background: var(--surface);
  color: var(--text-dim);
}

.badge.release {
  background: rgba(34, 197, 94, 0.16);
  color: #22c55e;
}

.badge.beta {
  background: rgba(245, 158, 11, 0.16);
  color: #f59e0b;
}

.badge.alpha {
  background: rgba(239, 68, 68, 0.16);
  color: #ef4444;
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

.locals {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  list-style: none;
}

.locals li {
  padding: 8px 10px;
  border-radius: var(--radius-sm);
}

.locals li:hover {
  background: var(--surface-hover);
}

.locals li.off .lname {
  color: var(--text-faint);
  text-decoration: line-through;
}

.lname {
  font-size: 13px;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.lmeta {
  display: flex;
  gap: 8px;
  font-size: 11px;
  color: var(--text-faint);
  margin-top: 3px;
}

.lacts {
  display: flex;
  gap: 6px;
  margin-top: 6px;
}

.ghost.tiny {
  padding: 4px 10px;
  font-size: 11px;
  border-radius: 6px;
}

.ghost.tiny.danger {
  border-color: rgba(239, 68, 68, 0.5);
  color: #ef4444;
}

.dir {
  font-size: 11px;
  font-family: var(--font-mono);
  color: var(--text-faint);
  word-break: break-all;
  padding: 0 4px;
  max-height: 44px;
  overflow: hidden;
}

.empty {
  font-size: 12px;
  color: var(--text-faint);
  line-height: 1.6;
  padding: 6px 4px;
}

.main {
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
  padding: 12px 14px;
  border-bottom: 1px solid var(--border);
}

.searchbox {
  flex: 1;
  min-width: 0;
}

.bar .title {
  font-size: 14px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.author {
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
}

.primary,
.ghost {
  padding: 8px 14px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  color: var(--text);
  white-space: nowrap;
}

.primary {
  border-color: transparent;
  background: var(--accent);
  color: #fff;
  font-weight: 600;
}

.primary:disabled,
.ghost:disabled {
  opacity: 0.55;
}

.ghost.small {
  padding: 4px 10px;
  font-size: 11px;
}

.list {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 10px 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.card {
  display: flex;
  gap: 12px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  cursor: pointer;
  transition: border-color 0.15s var(--ease), background 0.15s var(--ease);
}

.card:hover {
  border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
  background: var(--surface);
}

.icon {
  width: 48px;
  height: 48px;
  border-radius: var(--radius-sm);
  object-fit: cover;
  flex-shrink: 0;
  background: var(--surface);
}

.icon.blank {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  font-weight: 700;
  color: var(--text-faint);
}

.cbody {
  min-width: 0;
  flex: 1;
}

.row {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.row h3 {
  font-size: 14px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row .author {
  margin-left: auto;
}

.cbody p {
  margin: 4px 0 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-dim);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.meta {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 6px;
  font-size: 11px;
  color: var(--text-faint);
  flex-wrap: wrap;
}

.tagc {
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--surface);
}

.more {
  display: flex;
  justify-content: center;
  padding: 6px 0 10px;
}

.hint {
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-faint);
  margin: 0;
}

.hint.big {
  padding: 20px 16px;
  font-size: 13px;
}

.detail {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 14px 16px 20px;
}

.bigicon {
  width: 64px;
  height: 64px;
  border-radius: var(--radius-md);
  object-fit: cover;
  background: var(--surface);
}

.desc {
  margin: 10px 0 6px;
  font-size: 13px;
  line-height: 1.65;
  color: var(--text-dim);
}

.versions {
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 12px;
}

.versions li {
  padding: 10px 12px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
  background: var(--bg-elevated);
}

.vtop {
  display: flex;
  align-items: center;
  gap: 8px;
}

.vtop strong {
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.vmeta {
  display: flex;
  gap: 10px;
  margin-top: 5px;
  font-size: 11px;
  color: var(--text-faint);
  flex-wrap: wrap;
}

.mono {
  font-family: var(--font-mono);
}

.warn {
  margin: 6px 0 0;
  font-size: 11px;
  line-height: 1.5;
  color: #f59e0b;
}

.vacts {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
}

.picks {
  display: flex;
  gap: 6px;
}
.picks button {
  flex: 1;
}
.picks button.on {
  border-color: var(--accent);
  color: var(--accent);
}
.empty.small {
  font-size: 12px;
  padding: 8px 2px;
}
</style>