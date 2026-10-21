<script setup lang="ts">
defineProps<{ title: string; desc: string }>();
</script>

<template>
  <div class="placeholder">
    <div class="card">
      <h2>{{ title }}</h2>
      <p>{{ desc }}</p>
      <span class="badge">开发中</span>
    </div>
  </div>
</template>

<style scoped>
.placeholder {
  height: 100%;
  display: grid;
  place-items: center;
  padding: 24px;
}

.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow);
  padding: 32px 40px;
  text-align: center;
  max-width: 440px;
}

h2 {
  font-size: 17px;
  margin-bottom: 10px;
}

p {
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.7;
}

.badge {
  display: inline-block;
  margin-top: 16px;
  font-size: 11px;
  padding: 3px 10px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 15%, transparent);
  color: var(--accent);
}
</style>
