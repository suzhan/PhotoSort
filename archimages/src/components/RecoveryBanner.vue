<script setup lang="ts">
import { useI18n } from "vue-i18n";
import type { PendingJobSummary } from "../types/job";

const props = defineProps<{ jobs: PendingJobSummary[] }>();
const emit = defineEmits<{ abandon: [string] }>();
const { t } = useI18n();

function formatTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString();
}
</script>

<template>
  <section v-if="props.jobs.length" class="panel recovery">
    <p class="section-label">{{ t("recovery.title") }}</p>
    <p class="muted">{{ t("recovery.hint") }}</p>
    <div v-for="job in props.jobs" :key="job.jobId" class="recovery-item">
      <span>
        {{ formatTime(job.startedAt) }} —
        {{ job.sourceRoot ?? "?" }} → {{ job.destinationRoot ?? "?" }}
        ({{ job.finishedFiles }}/{{ job.totalFiles }})
      </span>
      <button type="button" @click="emit('abandon', job.jobId)">
        {{ t("recovery.abandon") }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.panel {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 14px 16px;
}

.recovery {
  border-color: var(--warn);
}

.section-label {
  font-weight: 600;
  margin: 4px 0;
}

.muted {
  color: var(--muted);
  margin: 4px 0;
}

.recovery-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-top: 6px;
  font-size: 13px;
  word-break: break-all;
}
</style>
