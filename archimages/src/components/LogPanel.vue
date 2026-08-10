<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useLogStore } from "../stores/log";

const { t } = useI18n();
const logs = useLogStore();
const listEl = ref<HTMLElement | null>(null);

// 新日志自动滚到底（用户上翻时顺其自然被顶离底部即可）
watch(
  () => logs.lines.length,
  async () => {
    await nextTick();
    const el = listEl.value;
    if (el) el.scrollTop = el.scrollHeight;
  },
);
</script>

<template>
  <div>
    <div class="log-header">
      <label class="section-label">{{ t("logs.title") }}</label>
      <button type="button" @click="logs.clear()">{{ t("logs.clear") }}</button>
    </div>
    <ul ref="listEl" class="log-list mono">
      <li v-for="(line, i) in logs.lines" :key="i">{{ line }}</li>
    </ul>
  </div>
</template>

<style scoped>
.log-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.section-label {
  font-weight: 600;
}

.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.log-list {
  max-height: 140px;
  overflow: auto;
  margin: 8px 0 0;
  padding: 8px 10px;
  list-style: none;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  font-size: 12px;
  word-break: break-all;
}
</style>
