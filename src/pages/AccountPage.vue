<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useAccountStore } from "../stores/account";
import { useNotifyStore } from "../stores/notify";
import type { Account, UuidMode } from "../api/account";
import { openInBrowser } from "../api/microsoft";

const store = useAccountStore();
const notify = useNotifyStore();

const selectedId = ref<string | null>(null);
const adding = ref(false);
const draftName = ref("");
const draftMode = ref<UuidMode>("name-based");

const editName = ref("");
const editMode = ref<UuidMode>("name-based");
const confirmDelete = ref(false);

/** 选中项失效（被删或首次进入）时回落到当前账户或第一个 */
const selected = computed<Account | null>(() => {
  const hit = selectedId.value
    ? store.accounts.find((a) => a.id === selectedId.value)
    : undefined;
  return hit ?? store.current ?? store.accounts[0] ?? null;
});

const dirty = computed(() => {
  const a = selected.value;
  if (!a) return false;
  return editName.value.trim() !== a.name || editMode.value !== a.uuid_mode;
});

function syncEdit() {
  const a = selected.value;
  editName.value = a?.name ?? "";
  editMode.value = a?.uuid_mode ?? "name-based";
  confirmDelete.value = false;
}

watch(() => selected.value?.id, syncEdit, { immediate: true });

onMounted(async () => {
  await store.load();
});

function initial(a: Account) {
  return (a.name || "?").slice(0, 1).toUpperCase();
}

/** 用 UUID 前 6 位当色相，同一个账户每次进来颜色都一样 */
function avatarStyle(a: Account) {
  const n = Number.parseInt((a.uuid || "888888").slice(0, 6), 16);
  const hue = Number.isNaN(n) ? 210 : n % 360;
  return { background: `hsl(${hue}, 55%, 42%)` };
}

function pick(id: string) {
  selectedId.value = id;
  adding.value = false;
}

function startAdd() {
  adding.value = !adding.value;
  draftName.value = "";
  draftMode.value = "name-based";
}

async function submitAdd() {
  const created = await store.add(draftName.value, draftMode.value);
  if (created) {
    adding.value = false;
    draftName.value = "";
    selectedId.value = created.id;
  }
}

async function submitEdit() {
  const a = selected.value;
  if (!a) return;
  const saved = await store.update(a.id, editName.value, editMode.value);
  if (saved) {
    selectedId.value = saved.id;
    syncEdit();
  }
}

async function doRemove() {
  const a = selected.value;
  if (!a) return;
  const ok = await store.remove(a.id);
  if (ok) {
    confirmDelete.value = false;
    selectedId.value = null;
  }
}

async function copyUuid() {
  const a = selected.value;
  if (!a) return;
  try {
    await navigator.clipboard.writeText(a.uuid);
    notify.info("UUID 已复制");
  } catch {
    notify.error("复制失败，可以手动选中这串 UUID");
  }
}

/** 登录成功后把选中项切到新账户，用户不用自己去找 */
watch(
  () => store.msDoneId,
  (id) => {
    if (id) {
      selectedId.value = id;
      adding.value = false;
    }
  },
);

async function copyCode() {
  const code = store.msTicket?.user_code;
  if (!code) return;
  try {
    await navigator.clipboard.writeText(code);
    notify.info("验证码已复制");
  } catch {
    notify.error("复制失败，可以照着屏幕手动输入");
  }
}

async function openMsBrowser() {
  const url = store.msTicket?.verification_uri;
  if (!url) return;
  try {
    await openInBrowser(url);
  } catch {
    notify.error(`打开浏览器失败，请手动访问 ${url}`);
  }
}
</script>

<template>
  <div class="account">
    <div
      class="secret-bar"
      :class="{ warn: store.secretStatus && !store.secretStatus.available }"
    >
      <span class="dot"></span>
      <span>{{ store.secretStatus?.note ?? "正在检查凭据存储…" }}</span>
    </div>

    <div class="layout">
      <aside class="list">
        <div class="list-head">
          <span>账户</span>
          <button class="ghost small" :disabled="store.loading" @click="store.load()">
            {{ store.loading ? "加载中" : "刷新" }}
          </button>
        </div>

        <div v-if="store.loading && store.accounts.length === 0" class="skeleton"></div>

        <ul v-else class="items">
          <li
            v-for="a in store.accounts"
            :key="a.id"
            :class="{ on: selected?.id === a.id }"
            @click="pick(a.id)"
          >
            <span class="avatar" :style="avatarStyle(a)">{{ initial(a) }}</span>
            <span class="it-name">{{ a.name }}</span>
            <span v-if="a.kind === 'microsoft'" class="badge ms">正版</span>
            <span v-if="a.is_current" class="badge">当前</span>
          </li>
        </ul>

        <button class="add-btn" :class="{ on: adding }" @click="startAdd">
          {{ adding ? "取消添加" : "＋ 添加离线账户" }}
        </button>
        <button class="ms-btn" :disabled="!!store.msTicket" @click="store.startMicrosoft()">
          {{ store.msTicket ? "等待浏览器确认…" : "使用微软账户登录" }}
        </button>
      </aside>

      <section class="detail">
        <div v-if="store.msTicket" class="card">
          <p class="title">用微软账户登录</p>
          <p class="tip">在浏览器里打开下面的网址，输入这串验证码</p>
          <div class="code-row">
            <code class="code">{{ store.msTicket.user_code }}</code>
            <button class="ghost small" @click="copyCode">复制验证码</button>
          </div>
          <div class="row">
            <label>网址</label>
            <code class="uuid">{{ store.msTicket.verification_uri }}</code>
            <button class="primary" @click="openMsBrowser">打开浏览器</button>
          </div>
          <p class="tip">{{ store.msMessage }}</p>
          <div class="actions">
            <button class="ghost" @click="store.cancelMicrosoft()">算了，取消登录</button>
          </div>
        </div>
        <div v-else-if="adding" class="card">
          <p class="title">添加离线账户</p>
          <p class="tip">不需要正版账号，填个名字就能进游戏</p>
          <div class="row">
            <label>用户名</label>
            <input
              v-model="draftName"
              maxlength="16"
              spellcheck="false"
              placeholder="3-16 位英文、数字或下划线"
              @keyup.enter="submitAdd"
            />
          </div>
          <div class="row col">
            <label>UUID 生成方式</label>
            <div class="seg">
              <button :class="{ on: draftMode === 'name-based' }" @click="draftMode = 'name-based'">
                按名字派生
              </button>
              <button :class="{ on: draftMode === 'random' }" @click="draftMode = 'random'">
                随机
              </button>
            </div>
            <p class="tip">
              和朋友一起玩离线服时选「按名字派生」，服务器那边算出来的 UUID 才能对上
            </p>
          </div>
          <div class="actions">
            <button class="primary" :disabled="store.busy || !draftName.trim()" @click="submitAdd">
              添加
            </button>
          </div>
        </div>

        <div v-else-if="!selected" class="card empty">
          <p class="title">还没有账户</p>
          <p class="tip">添加一个离线账户就能开始游戏，不需要正版账号</p>
          <button class="primary" @click="startAdd">添加离线账户</button>
        </div>

        <div v-else class="card">
          <div class="who">
            <span class="avatar big" :style="avatarStyle(selected)">{{ initial(selected) }}</span>
            <div>
              <p class="title">{{ selected.name }}</p>
              <p class="tip">{{ selected.kind === "offline" ? "离线账户" : "微软账户" }}</p>
            </div>
          </div>

          <div v-if="selected.kind === 'offline'" class="row">
            <label>用户名</label>
            <input v-model="editName" maxlength="16" spellcheck="false" />
            <button class="primary" :disabled="store.busy || !dirty" @click="submitEdit">
              保存
            </button>
          </div>

          <div class="row">
            <label>UUID</label>
            <code class="uuid">{{ selected.uuid }}</code>
            <button class="ghost small" @click="copyUuid">复制</button>
          </div>

          <div v-if="selected.kind === 'microsoft'" class="row">
            <label>登录状态</label>
            <span class="tip">
              {{
                selected.has_secret
                  ? "凭据已保存，启动时自动续期"
                  : "凭据丢了，需要重新登录一次"
              }}
            </span>
            <button
              v-if="!selected.has_secret"
              class="ghost small"
              :disabled="!!store.msTicket"
              @click="store.startMicrosoft()"
            >
              重新登录
            </button>
          </div>

          <div v-if="selected.kind === 'offline'" class="row col">
            <label>UUID 生成方式</label>
            <div class="seg">
              <button :class="{ on: editMode === 'name-based' }" @click="editMode = 'name-based'">
                按名字派生
              </button>
              <button :class="{ on: editMode === 'random' }" @click="editMode = 'random'">
                随机
              </button>
            </div>
            <p class="tip">
              按名字派生：与离线服务器一致，但改名字会换 UUID；随机：UUID 固定，改名字不丢存档
            </p>
          </div>

          <div class="row">
            <label>使用状态</label>
            <span class="tip">{{ selected.is_current ? "当前使用中" : "未在使用" }}</span>
            <button
              v-if="!selected.is_current"
              class="ghost"
              :disabled="store.busy"
              @click="store.setCurrent(selected.id)"
            >
              设为当前账户
            </button>
          </div>

          <div class="danger">
            <button v-if="!confirmDelete" class="danger-btn" @click="confirmDelete = true">
              删除这个账户
            </button>
            <template v-else>
              <span class="tip warn">只删本地账户配置，不碰存档</span>
              <button class="danger-btn solid" :disabled="store.busy" @click="doRemove">
                确认删除
              </button>
              <button class="ghost small" @click="confirmDelete = false">取消</button>
            </template>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.account {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  overflow: hidden;
}

.secret-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  color: var(--text-dim);
  font-size: 12px;
}

.secret-bar.warn {
  border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
}

.secret-bar .dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--accent);
  flex-shrink: 0;
}

.layout {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 12px;
}

.list {
  width: 240px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 10px;
}

.list-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
  color: var(--text-dim);
}

.items {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.items li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 8px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background 0.15s var(--ease);
}

.items li:hover {
  background: var(--surface-hover);
}

.items li.on {
  background: color-mix(in srgb, var(--accent) 16%, transparent);
}

.avatar {
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  font-size: 13px;
  font-weight: 600;
  flex-shrink: 0;
}

.avatar.big {
  width: 44px;
  height: 44px;
  font-size: 18px;
}

.it-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}

.badge {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 999px;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 18%, transparent);
}

/* 正版标记和“当前”用不同颜色，一眼能分开 */
.badge.ms {
  color: #3fb27f;
  background: color-mix(in srgb, #3fb27f 18%, transparent);
}

.skeleton {
  height: 40px;
  border-radius: var(--radius-sm);
  background: var(--surface-hover);
}

.add-btn {
  padding: 8px;
  border-radius: var(--radius-sm);
  border: 1px dashed var(--border);
  color: var(--text-dim);
  font-size: 13px;
}

.add-btn:hover,
.add-btn.on {
  color: var(--accent);
  border-color: var(--accent);
}

.ms-btn {
  padding: 8px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
  font-size: 13px;
  font-weight: 600;
}

.ms-btn:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 22%, transparent);
}

.ms-btn:disabled {
  opacity: 0.6;
  cursor: default;
}

.code-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

/* 验证码要一眼能读出来，字距拉开、字号加大 */
.code {
  flex: 1;
  padding: 10px 14px;
  border-radius: var(--radius-sm);
  background: var(--bg);
  border: 1px dashed var(--border);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 22px;
  font-weight: 600;
  letter-spacing: 4px;
  text-align: center;
  color: var(--accent);
  user-select: all;
}

.detail {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}

.card {
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.card.empty {
  align-items: flex-start;
}

.who {
  display: flex;
  align-items: center;
  gap: 12px;
}

.title {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}

.tip {
  margin: 0;
  font-size: 12px;
  color: var(--text-faint);
  line-height: 1.5;
}

.tip.warn {
  color: var(--accent);
}

.row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.row.col {
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
}

.row label {
  width: 88px;
  flex-shrink: 0;
  font-size: 13px;
  color: var(--text-dim);
}

input {
  flex: 1;
  min-width: 0;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--bg);
  color: var(--text);
  font-size: 13px;
  font-family: inherit;
}

input:focus {
  outline: none;
  border-color: var(--accent);
}

.uuid {
  flex: 1;
  min-width: 0;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg);
  border: 1px solid var(--border);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  color: var(--text-dim);
  overflow-x: auto;
  white-space: nowrap;
}

.seg {
  display: inline-flex;
  padding: 2px;
  border-radius: var(--radius-sm);
  background: var(--bg);
  border: 1px solid var(--border);
}

.seg button {
  padding: 5px 12px;
  border-radius: calc(var(--radius-sm) - 2px);
  font-size: 12px;
  color: var(--text-dim);
}

.seg button.on {
  background: color-mix(in srgb, var(--accent) 20%, transparent);
  color: var(--accent);
  font-weight: 600;
}

.actions {
  display: flex;
  gap: 8px;
}

.primary {
  padding: 7px 16px;
  border-radius: var(--radius-sm);
  background: var(--accent);
  color: #fff;
  font-size: 13px;
  font-weight: 600;
}

.primary:hover {
  background: var(--accent-hover);
}

.primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ghost {
  padding: 7px 12px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  color: var(--text-dim);
  font-size: 13px;
}

.ghost:hover {
  color: var(--text);
  background: var(--surface-hover);
}

.ghost.small {
  padding: 4px 9px;
  font-size: 12px;
}

.danger {
  margin-top: 4px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.danger-btn {
  padding: 7px 14px;
  border-radius: var(--radius-sm);
  border: 1px solid color-mix(in srgb, #e5484d 55%, var(--border));
  color: #e5484d;
  font-size: 13px;
}

.danger-btn:hover {
  background: color-mix(in srgb, #e5484d 12%, transparent);
}

.danger-btn.solid {
  background: #e5484d;
  color: #fff;
  border-color: #e5484d;
}
</style>
