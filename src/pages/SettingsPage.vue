<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useSettingsStore } from "../stores/settings";
import { useLaunchStore } from "../stores/launch";

const store = useSettingsStore();
const launcher = useLaunchStore();
const s = store.settings;
const active = ref("launch");

const categories = [
  { key: "launch", label: "启动设置" },
  { key: "ui", label: "界面设置" },
  { key: "game", label: "游戏显示" },
  { key: "window", label: "启动器窗口" },
  { key: "java", label: "Java 管理" },
  { key: "download", label: "下载设置" },
];

// 内存校验：低于 512 或高于 64G 都夹回安全值
// Java 页第一次打开才扫，避免每次进设置都 spawn 一堆进程
function rescan() {
  void launcher.listJava(true);
}

watch(active, (key) => {
  if (key === "java" && launcher.javas.length === 0 && !launcher.javaLoading) {
    void launcher.listJava();
  }
});

onMounted(() => {
  if (active.value === "java") void launcher.listJava();
});

const memoryWarn = computed(() => {
  // store.settings 已经是解包后的对象，不是 ref，别再多写一层 .value
  const m = s.max_memory_mb;
  if (!m || m < 512) return "建议不低于 512 MB，否则游戏可能无法启动";
  if (m > 65536) return "超出合理范围，已限制为 65536 MB";
  if (m > 16384) return "单版本分配超过 16 GB 通常没有收益，反而拖慢 GC";
  return "";
});
</script>

<template>
  <div class="settings">
    <aside class="cats">
      <button
        v-for="c in categories"
        :key="c.key"
        class="cat"
        :class="{ on: active === c.key }"
        @click="active = c.key"
      >
        {{ c.label }}
      </button>
    </aside>

    <section class="pane">
      <div class="card">
        <!-- 启动设置 -->
        <template v-if="active === 'launch'">
          <div class="field">
            <label>最大内存</label>
            <input v-model.number="s.max_memory_mb" type="number" min="512" step="512" class="num" />
            <span class="unit">MB</span>
          </div>
          <p v-if="memoryWarn" class="tip warn">{{ memoryWarn }}</p>

          <div class="field">
            <label>版本隔离</label>
            <div class="seg">
              <button :class="{ on: s.isolation === 'full' }" @click="s.isolation = 'full'">全隔离</button>
              <button :class="{ on: s.isolation === 'non-fake' }" @click="s.isolation = 'non-fake'">仅隔离伪造版本</button>
              <button :class="{ on: s.isolation === 'none' }" @click="s.isolation = 'none'">不隔离</button>
            </div>
          </div>
          <p class="tip">全隔离：每个版本独立 .minecraft；仅隔离伪造版本：加载器/整合包独立，原版共用；不隔离：全部共用一个目录</p>
        </template>

        <!-- 界面设置 -->
        <template v-else-if="active === 'ui'">
          <div class="field">
            <label>主题</label>
            <div class="seg">
              <button :class="{ on: s.theme === 'dark' }" @click="s.theme = 'dark'">深色</button>
              <button :class="{ on: s.theme === 'light' }" @click="s.theme = 'light'">浅色</button>
              <button :class="{ on: s.theme === 'system' }" @click="s.theme = 'system'">跟随系统</button>
            </div>
          </div>

          <div class="field">
            <label>强调色</label>
            <div class="accents">
              <button
                v-for="a in store.accents"
                :key="a"
                class="swatch"
                :class="[a, { on: s.accent === a }]"
                :title="a"
                @click="s.accent = a"
              />
            </div>
          </div>
        </template>

        <!-- 游戏显示：只影响游戏本身 -->
        <template v-else-if="active === 'game'">
          <div class="field">
            <label>游戏分辨率</label>
            <input v-model.number="s.window_width" type="number" min="320" class="num" />
            <span class="unit">×</span>
            <input v-model.number="s.window_height" type="number" min="240" class="num" />
          </div>
          <p class="tip">拼进游戏启动参数 --width / --height，仅在有窗口时生效</p>

          <div class="field">
            <label>全屏启动</label>
            <div class="seg">
              <button :class="{ on: s.fullscreen_mode === 'ignore' }" @click="s.fullscreen_mode = 'ignore'">不修改</button>
              <button :class="{ on: s.fullscreen_mode === 'on' }" @click="s.fullscreen_mode = 'on'">全屏</button>
              <button :class="{ on: s.fullscreen_mode === 'off' }" @click="s.fullscreen_mode = 'off'">窗口化</button>
            </div>
          </div>
          <p class="tip">
            原版没有全屏启动参数，只能在启动前改该版本的 options.txt；「不修改」表示完全不动，沿用你在游戏里自己的设置
          </p>
        </template>

        <!-- 启动器窗口：只管启动器自己 -->
        <template v-else-if="active === 'window'">
          <div class="field">
            <label>窗口尺寸</label>
            <input v-model.number="s.launcher_width" type="number" min="640" class="num" />
            <span class="unit">×</span>
            <input v-model.number="s.launcher_height" type="number" min="480" class="num" />
          </div>
          <div class="field">
            <label>启动时最大化</label>
            <button
              class="toggle"
              :class="{ on: s.launcher_maximized }"
              @click="s.launcher_maximized = !s.launcher_maximized"
            >
              {{ s.launcher_maximized ? "已开启" : "已关闭" }}
            </button>
          </div>
          <p class="tip">保存后立即生效；最大化开启时窗口尺寸只在取消最大化后使用</p>
        </template>

        <!-- Java 管理 -->
        <template v-else-if="active === 'java'">
          <div class="field">
            <label>手动指定</label>
            <input
              :value="s.java_path ?? ''"
              class="path"
              placeholder="java 可执行文件路径，留空则自动探测"
              @input="s.java_path = ($event.target as HTMLInputElement).value || null"
            />
            <button class="ghost small" :disabled="launcher.javaLoading" @click="rescan">
              {{ launcher.javaLoading ? "扫描中" : "重新扫描" }}
            </button>
          </div>
          <p class="tip">
            会扫 PATH、JAVA_HOME、各平台常见安装目录，以及上面手动填的路径；启动时自动挑一份够用的版本
          </p>

          <div class="java-list">
            <div class="jl-head">
              <span>本机检测到 {{ launcher.javas.length }} 个 Java</span>
            </div>
            <ul>
              <li v-for="j in launcher.javas" :key="j.path">
                <span class="jl-major">Java {{ j.major }}</span>
                <span class="jl-ver">{{ j.version }}</span>
                <span class="jl-src">{{ j.source }}</span>
                <code class="jl-path" :title="j.path">{{ j.path }}</code>
              </li>
            </ul>
            <p v-if="!launcher.javaLoading && launcher.javas.length === 0" class="tip">
              没检测到 Java；到启动页选一个版本后点「一键下载」，会从官方清单装一份回来
            </p>
          </div>
        </template>
        <!-- 下载设置 -->
        <template v-else>
          <div class="field">
            <label>优先镜像</label>
            <button class="toggle" :class="{ on: s.prefer_mirror }" @click="s.prefer_mirror = !s.prefer_mirror">
              {{ s.prefer_mirror ? "BMCLAPI 优先" : "官方源优先" }}
            </button>
          </div>
          <div class="field">
            <label>切换阈值</label>
            <input v-model.number="s.mirror_timeout_secs" type="number" min="1" class="num" />
            <span class="unit">秒</span>
          </div>
          <p class="tip">官方源超过该时长未响应则自动切换镜像源</p>
        </template>
      </div>
    </section>
  </div>
</template>

<style scoped>
.settings {
  height: 100%;
  display: grid;
  grid-template-columns: 180px 1fr;
  gap: 14px;
  padding: 16px;
}

.cats {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.cat {
  padding: 10px 12px;
  border-radius: var(--radius-sm);
  text-align: left;
  color: var(--text-dim);
  font-size: 13px;
  transition: background 0.14s var(--ease), color 0.14s var(--ease);
}

.cat:hover {
  background: var(--surface-hover);
  color: var(--text);
}

.cat.on {
  background: color-mix(in srgb, var(--accent) 16%, transparent);
  color: var(--accent);
  font-weight: 600;
}

.pane {
  overflow-y: auto;
}

.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow);
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.field {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.field label {
  width: 90px;
  font-size: 13px;
  color: var(--text-dim);
  flex-shrink: 0;
}

.tip {
  font-size: 12px;
  color: var(--text-faint);
  line-height: 1.6;
  margin-top: -8px;
}

.tip.warn {
  color: #f59e0b;
}

.seg {
  display: flex;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  overflow: hidden;
}

.seg button {
  padding: 6px 14px;
  font-size: 12px;
  color: var(--text-dim);
  transition: background 0.14s var(--ease);
}

.seg button:hover {
  background: var(--surface-hover);
}

.seg button.on {
  background: var(--accent);
  color: #fff;
}

.accents {
  display: flex;
  gap: 8px;
}

.swatch {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  border: 2px solid transparent;
  transition: transform 0.12s var(--ease);
}

.swatch:hover {
  transform: scale(1.12);
}

.swatch.on {
  border-color: var(--text);
}

.swatch.blue { background: #3b82f6; }
.swatch.purple { background: #8b5cf6; }
.swatch.green { background: #22c55e; }
.swatch.orange { background: #f97316; }
.swatch.pink { background: #ec4899; }
.swatch.red { background: #ef4444; }
.swatch.cyan { background: #06b6d4; }
.swatch.amber { background: #f59e0b; }

.num {
  width: 110px;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  color: var(--text);
  font-size: 13px;
}

.path {
  flex: 1;
  min-width: 240px;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--bg-elevated);
  color: var(--text);
  font-size: 13px;
  font-family: var(--font-mono);
}

.toggle {
  padding: 6px 14px;
  font-size: 12px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  color: var(--text-dim);
}

.toggle.on {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}

.unit {
  font-size: 12px;
  color: var(--text-faint);
}

.java-list {
  margin-top: 4px;
  border-top: 1px solid var(--border);
  padding-top: 10px;
}

.jl-head {
  font-size: 12px;
  color: var(--text-faint);
  margin-bottom: 6px;
}

.java-list ul {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.java-list li {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  font-size: 12px;
}

.jl-major {
  flex-shrink: 0;
  color: var(--accent);
  font-weight: 600;
}

.jl-ver {
  flex-shrink: 0;
  color: var(--text-dim);
}

.jl-src {
  flex-shrink: 0;
  color: var(--text-faint);
  font-size: 11px;
}

.jl-path {
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
</style>
