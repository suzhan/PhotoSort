<script setup lang="ts">
import { pickDirectory } from "../services/tauri";
import { useLogStore } from "../stores/log";
import { useI18n } from "vue-i18n";

const props = defineProps<{
  label: string;
  placeholder: string;
  modelValue: string;
}>();
const emit = defineEmits<{ "update:modelValue": [string] }>();

const { t } = useI18n();
const logs = useLogStore();

async function browse() {
  try {
    const dir = await pickDirectory();
    if (dir) emit("update:modelValue", dir);
  } catch (e) {
    logs.push(`${t("scan.pickFailed")}：${String(e)}`);
  }
}

function onInput(event: Event) {
  emit("update:modelValue", (event.target as HTMLInputElement).value);
}
</script>

<template>
  <div class="row">
    <label class="field-label">{{ props.label }}</label>
    <input
      class="path-input"
      :value="props.modelValue"
      :placeholder="props.placeholder"
      @input="onInput"
    />
    <button type="button" @click="browse">{{ t("scan.browse") }}</button>
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

.path-input {
  flex: 1;
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg);
  color: var(--text);
}
</style>
