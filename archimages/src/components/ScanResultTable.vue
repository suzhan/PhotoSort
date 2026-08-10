<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import type { ScanRow } from "../types/scan";

/**
 * 手写虚拟滚动：固定行高 + 可视窗口 + 上下各 5 行缓冲。
 * 10 万行只渲染 ~20 个 DOM 节点（需求 §二十七）。
 */
const props = defineProps<{ rows: ScanRow[] }>();
const { t } = useI18n();

const ROW_HEIGHT = 28;
const VIEWPORT_HEIGHT = 336;
const BUFFER = 5;

const scrollTop = ref(0);

const visibleCount = Math.ceil(VIEWPORT_HEIGHT / ROW_HEIGHT) + BUFFER * 2;
const start = computed(() =>
  Math.max(0, Math.floor(scrollTop.value / ROW_HEIGHT) - BUFFER),
);
const end = computed(() =>
  Math.min(props.rows.length, start.value + visibleCount),
);
const windowRows = computed(() => props.rows.slice(start.value, end.value));

function onScroll(event: Event) {
  scrollTop.value = (event.target as HTMLElement).scrollTop;
}

function statusClass(status: string): string {
  if (status === "ready") return "st-ready";
  if (status === "duplicate") return "st-duplicate";
  if (status === "error" || status === "collision") return "st-bad";
  return "st-warn";
}
</script>

<template>
  <div class="table">
    <div class="thead">
      <span class="col-source">{{ t("table.source") }}</span>
      <span class="col-target">{{ t("table.target") }}</span>
      <span class="col-status">{{ t("table.status") }}</span>
    </div>
    <div
      class="viewport"
      :style="{ height: VIEWPORT_HEIGHT + 'px' }"
      @scroll="onScroll"
    >
      <div
        class="spacer"
        :style="{ height: props.rows.length * ROW_HEIGHT + 'px' }"
      >
        <div
          v-for="(row, i) in windowRows"
          :key="row.seq"
          class="trow"
          :style="{ top: (start + i) * ROW_HEIGHT + 'px' }"
        >
          <span class="col-source mono" :title="row.sourcePath">
            {{ row.sourcePath }}
          </span>
          <span class="col-target mono" :title="row.targetPath ?? ''">
            {{ row.targetPath ?? "—" }}
          </span>
          <span class="col-status" :class="statusClass(row.status)">
            {{ t(`status.${row.status}`) }}
          </span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.table {
  border: 1px solid var(--border);
  border-radius: 6px;
  overflow: hidden;
  margin-top: 8px;
}

.thead {
  display: flex;
  gap: 12px;
  padding: 6px 10px;
  background: var(--bg);
  border-bottom: 1px solid var(--border);
  font-weight: 600;
  font-size: 12px;
}

.viewport {
  overflow-y: auto;
  position: relative;
}

.spacer {
  position: relative;
}

.trow {
  position: absolute;
  left: 0;
  right: 0;
  height: 28px;
  display: flex;
  gap: 12px;
  align-items: center;
  padding: 0 10px;
  font-size: 12px;
  border-bottom: 1px solid var(--border);
  box-sizing: border-box;
}

.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.col-source,
.col-target {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.col-status {
  width: 110px;
  flex-shrink: 0;
  text-align: right;
}

.st-ready {
  color: var(--ok);
}

.st-duplicate {
  color: var(--accent);
}

.st-warn {
  color: var(--warn);
}

.st-bad {
  color: var(--error);
}
</style>
