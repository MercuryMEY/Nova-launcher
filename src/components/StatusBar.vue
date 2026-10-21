<script setup lang="ts">
import { useNotifyStore } from "../stores/notify";
const notify = useNotifyStore();
</script>

<template>
  <footer class="statusbar">
    <TransitionGroup name="notice">
      <div
        v-for="n in notify.items"
        :key="n.id"
        class="notice"
        :class="n.kind"
        @click="notify.dismiss(n.id)"
      >
        {{ n.text }}
      </div>
    </TransitionGroup>
  </footer>
</template>

<style scoped>
.statusbar {
  height: 32px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 12px;
  background: var(--bg-elevated);
  border-top: 1px solid var(--border);
  overflow: hidden;
}

.notice {
  font-size: 12px;
  padding: 3px 10px;
  border-radius: 999px;
  cursor: pointer;
  white-space: nowrap;
  background: var(--surface);
  border: 1px solid var(--border);
  color: var(--text-dim);
}

.notice.success {
  color: #22c55e;
  border-color: color-mix(in srgb, #22c55e 40%, transparent);
}

.notice.error {
  color: #ef4444;
  border-color: color-mix(in srgb, #ef4444 40%, transparent);
}

.notice-enter-active,
.notice-leave-active {
  transition: all 0.2s var(--ease);
}
.notice-enter-from,
.notice-leave-to {
  opacity: 0;
  transform: translateY(6px);
}
</style>
