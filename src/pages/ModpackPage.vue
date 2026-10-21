<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import type { ModpackInstance } from "../api/modpack";
import { useModpackStore } from "../stores/modpack";
import { useLaunchStore } from "../stores/launch";

const packs = useModpackStore();
const launch = useLaunchStore();

/** 默认顺手把加载器装上：整合包没加载器基本跑不起来 */
const installLoader = ref(true);
const logBox = ref<HTMLElement | null>(null);

const canImport = computed(() => !!packs.preview && !packs.importing);

onMounted(async () => {
  packs.bind();
  await packs.refresh();
});

/**
 * 删除二次确认：点一下变红，3 秒内再点才真删。
 * 整合包目录里可能有几十个 Mod，误删一次重新下载就是几十分钟。
 */
const armed = ref("");
let armTimer = 0;

function askRemove(i: ModpackInstance) {
  if (armed.value === i.id) {
    window.clearTimeout(armTimer);
    armed.value = "";
    void packs.remove(i.id);
    return;
  }
  armed.value = i.id;
  window.clearTimeout(armTimer);
  armTimer = window.setTimeout(() => {
    armed.value = "";
  }, 3000);
}

onUnmounted(() => window.clearTimeout(armTimer));

/** 日志一直往下滚，用户不用手动拖 */
watch(
  () => packs.logs.length,
  async () => {
    await nextTick();
    const el = logBox.value;
    if (el) el.scrollTop = el.scrollHeight;
  },
);

async function choose() {
  const path = await packs.pick();
  if (!path) return;
  await packs.inspect(path);
}

function sizeText(n: number) {
  if (n <= 0) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** 后端只存 Unix 秒，格式化交给前端，省得后端为了一个时间戳引依赖 */
function dateText(sec: string) {
  const t = Number(sec);
  if (!t) return "";
  const d = new Date(t * 1000);
  const pad = (n: number) => `${n}`.padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** 加载器没装成的实例启动不了，按钮直接灰掉比点了报错友好 */
function canStart(i: ModpackInstance) {
  return !!i.launch_id && !launch.running && !launch.launching;
}

function startInstance(i: ModpackInstance) {
  if (!i.launch_id) return;
  // 传两个 id：launch_id 找版本，i.id 找实例目录（存档/Mod/配置在这层）
  void launch.launch(i.launch_id, i.id);
}
</script>

<template>
  <div class="pack">
    <aside class="side">
      <div class="head">
        <span>已安装 {{ packs.instances.length ? `(${packs.instances.length})` : "" }}</span>
        <button class="ghost small" :disabled="packs.importing" @click="packs.refresh()">刷新</button>
      </div>
      <ul class="insts">
        <li v-for="i in packs.instances" :key="i.id" :class="{ armed: armed === i.id }">
          <div class="iname" :title="i.name">{{ i.name }}</div>
          <div class="imeta">
            <span>{{ i.game }}</span>
            <span v-if="i.loader_version">{{ i.loader_version }}</span>
            <span>{{ i.file_count }} 个文件</span>
          </div>
          <div class="iline">
            <span v-if="!i.launch_id" class="warn">加载器没装</span>
            <span v-else-if="i.failed?.length" class="warn">缺 {{ i.failed.length }} 个文件</span>
            <span v-else class="dim">{{ dateText(i.created) }}</span>
            <span class="ops">
              <button
                v-if="i.failed?.length"
                class="ghost small"
                :disabled="!!packs.retrying"
                @click="packs.retry(i.id)"
              >
                {{ packs.retrying === i.id ? "补齐中" : "补齐文件" }}
              </button>
              <button class="ghost small" :disabled="!canStart(i)" @click="startInstance(i)">
                {{ launch.running ? "运行中" : "启动" }}
              </button>
              <button class="ghost small danger" @click="askRemove(i)">
                {{ armed === i.id ? "确认删除" : "删除" }}
              </button>
            </span>
          </div>
        </li>
        <li v-if="!packs.instances.length" class="empty">还没有整合包</li>
      </ul>
    </aside>

    <section class="main">
      <div class="bar">
        <button class="primary" :disabled="packs.importing" @click="choose">
          {{ packs.preview ? "换一个整合包" : "选择整合包" }}
        </button>
        <span class="hint">只支持 Modrinth 的 .mrpack</span>
      </div>

      <div v-if="packs.inspecting" class="card dim">正在读取索引…</div>

      <div v-else-if="packs.preview" class="card">
        <div class="title">
          <h3>{{ packs.preview.name }}</h3>
          <span class="badge">{{ packs.preview.id }}</span>
        </div>
        <p v-if="packs.preview.summary" class="summary">{{ packs.preview.summary }}</p>
        <dl class="facts">
          <div>
            <dt>游戏版本</dt>
            <dd>{{ packs.preview.game }}</dd>
          </div>
          <div>
            <dt>加载器</dt>
            <dd>
              <template v-if="packs.preview.loader_label">
                {{ packs.preview.loader_label }} {{ packs.preview.loader_version }}
              </template>
              <template v-else>未声明</template>
            </dd>
          </div>
          <div>
            <dt>文件</dt>
            <dd>{{ packs.preview.file_count }} 个 · {{ sizeText(packs.preview.total_size) }}</dd>
          </div>
          <div>
            <dt>实例目录</dt>
            <dd class="mono" :title="packs.preview.dir">{{ packs.preview.dir }}</dd>
          </div>
        </dl>

        <p v-if="packs.preview.existing" class="tip">
          已经有同名实例了，这次会另存为 <b>{{ packs.preview.id }}</b>
        </p>
        <p v-if="!packs.preview.base_installed" class="tip warn">
          {{ packs.preview.game }} 原版还没装，加载器这步会跳过；导入完成后去下载页补装原版
        </p>

        <label class="check" :class="{ off: !packs.preview.base_installed }">
          <input
            v-model="installLoader"
            type="checkbox"
            :disabled="!packs.preview.base_installed || packs.importing"
          />
          顺便安装加载器
        </label>

        <div class="actions">
          <button class="primary" :disabled="!canImport" @click="packs.importPack(installLoader)">
            {{ packs.importing ? "导入中…" : "开始导入" }}
          </button>
          <button class="ghost" :disabled="packs.importing" @click="packs.preview = null">
            取消
          </button>
        </div>
      </div>

      <div v-else class="card empty">
        <p>选一个 .mrpack，先看预览再决定要不要导</p>
        <p class="dim small">打包好的整合包一般叫 <span class="mono">xxx.mrpack</span></p>
      </div>

      <div v-if="packs.importing || packs.logs.length" class="progress">
        <div class="pstage">{{ packs.stage || "准备中" }}</div>
        <div class="bar-track">
          <div class="bar-fill" :style="{ width: `${packs.percent}%` }"></div>
        </div>
        <div class="pmeta">
          <span>{{ packs.percent }}%</span>
          <span v-if="packs.total">{{ sizeText(packs.done) }} / {{ sizeText(packs.total) }}</span>
          <span v-else class="dim">正在计算总量</span>
        </div>
        <div ref="logBox" class="logs">
          <div v-for="(l, idx) in packs.logs" :key="idx" class="line">{{ l }}</div>
        </div>
      </div>

      <div v-if="packs.last" class="card done">
        <h3>{{ packs.last.instance.name }} 导入完成</h3>
        <p class="dim">
          用时 {{ packs.last.seconds }}s · 下载 {{ packs.last.downloaded }} · 已有
          {{ packs.last.skipped }} · 失败 {{ packs.last.failed }} · 覆盖文件
          {{ packs.last.overrides }}
        </p>
        <p v-if="packs.last.failed > 0" class="tip warn">
          有 {{ packs.last.failed }} 个文件没下下来，进游戏可能缺内容；重导一次会只补缺的
        </p>
        <p v-if="!packs.last.launch_id" class="tip warn">
          加载器还没装，这个实例暂时启动不了；去下载页把加载器装好再来
        </p>
      </div>
    </section>
  </div>
</template>

<style scoped>
.pack {
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

.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-faint);
}

.insts {
  list-style: none;
  margin: 0;
  padding: 0;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.insts li {
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--surface);
  transition: background 0.15s var(--ease);
}

.insts li:hover {
  background: var(--surface-hover);
}

.insts li.armed {
  border-color: var(--accent);
}

.iname {
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.imeta {
  display: flex;
  gap: 8px;
  font-size: 11px;
  color: var(--text-faint);
  margin-top: 3px;
}

.iline {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 6px;
  font-size: 11px;
}

.ops {
  display: flex;
  gap: 4px;
}

.empty {
  font-size: 12px;
  color: var(--text-faint);
  text-align: center;
  padding: 18px 0;
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
  gap: 10px;
}

.hint {
  font-size: 12px;
  color: var(--text-faint);
}

.card {
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--surface);
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.title h3,
.done h3 {
  margin: 0;
  font-size: 15px;
}

.summary {
  margin: 0;
  font-size: 13px;
  color: var(--text-dim);
  line-height: 1.6;
}

.facts {
  margin: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
  gap: 8px 16px;
}

.facts div {
  min-width: 0;
}

.facts dt {
  font-size: 11px;
  color: var(--text-faint);
}

.facts dd {
  margin: 2px 0 0;
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mono {
  font-family: var(--font-mono);
  font-size: 11px;
}

.badge {
  font-size: 11px;
  padding: 2px 6px;
  border-radius: 999px;
  border: 1px solid var(--border);
  color: var(--text-faint);
  font-family: var(--font-mono);
}

.tip {
  margin: 0;
  font-size: 12px;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-elevated);
  color: var(--text-dim);
}

.tip.warn,
.warn {
  color: var(--accent);
}

.check {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  cursor: pointer;
}

.check.off {
  color: var(--text-faint);
  cursor: not-allowed;
}

.actions {
  display: flex;
  gap: 8px;
}

.progress {
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--surface);
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.pstage {
  font-size: 13px;
}

.bar-track {
  height: 6px;
  border-radius: 3px;
  background: var(--bg-elevated);
  overflow: hidden;
}

.bar-fill {
  height: 100%;
  background: var(--accent);
  transition: width 0.2s var(--ease);
}

.pmeta {
  display: flex;
  justify-content: space-between;
  font-size: 11px;
  color: var(--text-dim);
}

.logs {
  max-height: 200px;
  overflow-y: auto;
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.7;
  color: var(--text-dim);
  background: var(--bg);
  border-radius: var(--radius-sm);
  padding: 8px 10px;
}

.line {
  white-space: pre-wrap;
  word-break: break-all;
}

button.primary {
  padding: 8px 16px;
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  background: var(--accent);
  color: #fff;
  font-size: 13px;
  font-family: inherit;
  cursor: pointer;
}

button.primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

button.ghost {
  padding: 8px 14px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text);
  font-size: 13px;
  font-family: inherit;
  cursor: pointer;
}

button.ghost:hover:not(:disabled) {
  background: var(--surface-hover);
}

button.ghost:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

button.small {
  padding: 3px 8px;
  font-size: 11px;
}

button.danger {
  color: var(--accent);
}

.dim {
  color: var(--text-faint);
}
</style>
