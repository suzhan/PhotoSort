<script setup lang="ts">
import { ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { templatePreview } from "../services/tauri";
import type { TemplatePreview } from "../types/template";

const props = defineProps<{
  directoryTemplate: string;
  filenameTemplate: string;
}>();
const emit = defineEmits<{
  "update:directoryTemplate": [string];
  "update:filenameTemplate": [string];
}>();

const { t } = useI18n();
const preview = ref<TemplatePreview | null>(null);
const previewError = ref("");

// 本地镜像：输入立即更新本地值，防抖后用本地值请求预览。
// 父组件可能不回写 prop（单向流），本地值保证 IPC 参数正确。
const dirLocal = ref(props.directoryTemplate);
const fileLocal = ref(props.filenameTemplate);
watch(() => props.directoryTemplate, (v) => (dirLocal.value = v));
watch(() => props.filenameTemplate, (v) => (fileLocal.value = v));

let timer: ReturnType<typeof setTimeout> | undefined;

/** 输入防抖 300ms，避免每敲一个字符打一次 IPC。 */
function schedulePreview() {
  clearTimeout(timer);
  timer = setTimeout(refresh, 300);
}

async function refresh() {
  try {
    preview.value = await templatePreview({
      directoryTemplate: dirLocal.value,
      filenameTemplate: fileLocal.value,
    });
    previewError.value = "";
  } catch (e) {
    preview.value = null;
    previewError.value = String(e);
  }
}

function onDir(event: Event) {
  dirLocal.value = (event.target as HTMLInputElement).value;
  emit("update:directoryTemplate", dirLocal.value);
  schedulePreview();
}

function onFile(event: Event) {
  fileLocal.value = (event.target as HTMLInputElement).value;
  emit("update:filenameTemplate", fileLocal.value);
  schedulePreview();
}

defineExpose({ refresh });
</script>

<template>
  <div>
    <div class="row">
      <label class="field-label">{{ t("rules.directoryTemplate") }}</label>
      <input
        class="path-input mono"
        :value="dirLocal"
        placeholder="{yyyy}/{camera_model}"
        @input="onDir"
      />
    </div>
    <div class="row">
      <label class="field-label">{{ t("rules.filenameTemplate") }}</label>
      <input
        class="path-input mono"
        :value="fileLocal"
        placeholder="{yyyyMMdd}_{HHmmss}_{seq:4}.{extension}"
        @input="onFile"
      />
    </div>
    <p v-if="preview" class="muted mono preview">{{ preview.example }}</p>
    <p v-if="previewError" class="error">
      {{ t("rules.invalid") }}：{{ previewError }}
    </p>
  </div>
</template>

<style scoped>
.row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 8px;
}

.field-label {
  min-width: 96px;
  color: var(--muted);
}

.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.path-input {
  flex: 1;
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg);
  color: var(--text);
}

.preview {
  margin-top: 8px;
  word-break: break-all;
}

.error {
  color: var(--error);
  word-break: break-all;
}
</style>
