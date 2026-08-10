<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import type { ProgressEvent } from "../types/task";

const props = defineProps<{
  progress: ProgressEvent | null;
  active: boolean;
}>();
const { t } = useI18n();

const percent = computed(() =>
  props.progress ? props.progress.percent.toFixed(1) : "0.0",
);
</script>

<template>
  <div v-if="props.active && props.progress" class="progress-panel">
    <div class="progress-row">
      <div class="progress-track">
        <div class="progress-fill" :style="{ width: percent + '%' }"></div>
      </div>
      <span class="muted">
        {{ props.progress.current }} / {{ props.progress.total }} ·
        {{ percent }}%
      </span>
    </div>
    <p class="muted counts">
      {{
        t("organize.counts", {
          success: props.progress.success,
          duplicate: props.progress.duplicate,
          skipped: props.progress.skipped,
          failed: props.progress.failed,
        })
      }}
    </p>
    <p v-if="props.progress.currentFile" class="muted mono current">
      {{ props.progress.currentFile }}
    </p>
  </div>
</template>

<style scoped>
.progress-panel {
  margin-top: 10px;
}

.progress-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.progress-track {
  flex: 1;
  height: 8px;
  background: var(--border);
  border-radius: 4px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--accent);
  transition: width 0.2s ease;
}

.counts {
  margin: 6px 0 0;
}

.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.current {
  font-size: 12px;
  word-break: break-all;
  margin: 4px 0 0;
}
</style>
