<script setup lang="ts">
import { useI18n } from "vue-i18n";
import type { OperationMode } from "../types/settings";

const props = defineProps<{ modelValue: OperationMode }>();
const emit = defineEmits<{ "update:modelValue": [OperationMode] }>();
const { t } = useI18n();

const modes: OperationMode[] = ["copyVerifyDelete", "copy", "move"];
</script>

<template>
  <div class="modes" role="radiogroup" :aria-label="t('mode.title')">
    <label v-for="m in modes" :key="m" class="radio">
      <input
        type="radio"
        name="operation-mode"
        :value="m"
        :checked="props.modelValue === m"
        @change="emit('update:modelValue', m)"
      />
      <span>{{ t(`mode.${m}`) }}</span>
      <span class="muted hint">{{ t(`mode.${m}Hint`) }}</span>
    </label>
  </div>
</template>

<style scoped>
.modes {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-top: 8px;
}

.radio {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.hint {
  font-size: 12px;
}
</style>
