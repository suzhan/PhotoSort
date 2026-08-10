<script setup lang="ts">
import { ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { AppSettings } from "../types/settings";

/**
 * 高级设置对话框：操作的是副本，保存才提交。
 * 校验规则与 Rust 侧 AppSettings::validate 保持一致（1..=16 worker 等）。
 */
const props = defineProps<{
  open: boolean;
  settings: AppSettings | null;
  apiKeyConfigured: boolean;
  testing: boolean;
  testResult: string;
}>();
const emit = defineEmits<{
  close: [];
  save: [AppSettings];
  "save-key": [string];
  "clear-key": [];
  "test-geocode": [];
}>();

const { t } = useI18n();
const local = ref<AppSettings | null>(null);
const error = ref("");
const keyInput = ref("");

watch(
  () => [props.open, props.settings] as const,
  ([open, s]) => {
    if (open && s) {
      local.value = JSON.parse(JSON.stringify(s)) as AppSettings;
      error.value = "";
    }
  },
  { immediate: true },
);

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, Math.round(n)));
}

function save() {
  const s = local.value;
  if (!s) return;
  s.maxCopyWorkers = clamp(s.maxCopyWorkers, 1, 16);
  s.maxHashWorkers = clamp(s.maxHashWorkers, 1, 16);
  s.gpsRoundPrecision = clamp(s.gpsRoundPrecision, 2, 6);
  if (!s.metadataFallback.unknownCamera.trim()) {
    error.value = t("settings.invalidUnknown");
    return;
  }
  if (!s.metadataFallback.unknownLocation.trim()) {
    error.value = t("settings.invalidUnknown");
    return;
  }
  if (!s.metadataFallback.unknownDate.trim()) {
    error.value = t("settings.invalidUnknown");
    return;
  }
  emit("save", s);
}
</script>

<template>
  <div v-if="props.open && local" class="overlay" @click.self="emit('close')">
    <div class="dialog" role="dialog" :aria-label="t('settings.title')">
      <h2>{{ t("settings.title") }}</h2>

      <div class="field">
        <label>{{ t("settings.duplicateMode") }}</label>
        <select v-model="local.duplicateMode">
          <option value="modern">{{ t("settings.duplicateModern") }}</option>
          <option value="legacyStrict">
            {{ t("settings.duplicateLegacy") }}
          </option>
        </select>
      </div>

      <div class="field">
        <label>{{ t("settings.theme") }}</label>
        <select v-model="local.theme">
          <option value="system">{{ t("settings.themeSystem") }}</option>
          <option value="light">{{ t("settings.themeLight") }}</option>
          <option value="dark">{{ t("settings.themeDark") }}</option>
        </select>
      </div>

      <div class="field">
        <label>{{ t("settings.workers") }}</label>
        <div class="inline">
          <span class="muted">{{ t("settings.copyWorkers") }}</span>
          <input type="number" v-model.number="local.maxCopyWorkers" min="1" max="16" />
          <span class="muted">{{ t("settings.hashWorkers") }}</span>
          <input type="number" v-model.number="local.maxHashWorkers" min="1" max="16" />
        </div>
      </div>

      <div class="field">
        <label class="checkbox">
          <input type="checkbox" v-model="local.metadataFallback.useModifiedTime" />
          {{ t("settings.useModifiedTime") }}
        </label>
      </div>

      <div class="field">
        <label>{{ t("settings.fallbackNames") }}</label>
        <div class="inline">
          <input v-model="local.metadataFallback.unknownCamera" :placeholder="t('settings.unknownCamera')" />
          <input v-model="local.metadataFallback.unknownLocation" :placeholder="t('settings.unknownLocation')" />
          <input v-model="local.metadataFallback.unknownDate" :placeholder="t('settings.unknownDate')" />
        </div>
      </div>

      <div class="field">
        <label class="checkbox">
          <input type="checkbox" v-model="local.gpsEnabled" />
          {{ t("settings.gpsEnabled") }}
        </label>
      </div>
      <div v-if="local.gpsEnabled" class="field">
        <label>{{ t("settings.gpsNoApi") }}</label>
        <select v-model="local.gpsNoApiMode">
          <option value="ignore">{{ t("settings.gpsIgnore") }}</option>
          <option value="coordinates">{{ t("settings.gpsCoordinates") }}</option>
          <option value="unknownLocation">{{ t("settings.gpsUnknown") }}</option>
        </select>
        <p class="muted hint">{{ t("settings.gpsNoApiHint") }}</p>
      </div>

      <div class="field">
        <label>{{ t("settings.apiKey") }}</label>
        <div class="inline">
          <input
            type="password"
            v-model="keyInput"
            :placeholder="apiKeyConfigured ? t('settings.apiKeySet') : t('settings.apiKeyPlaceholder')"
          />
          <button type="button" @click="emit('save-key', keyInput); keyInput = ''">
            {{ t("settings.apiKeySave") }}
          </button>
          <button v-if="apiKeyConfigured" type="button" @click="emit('clear-key')">
            {{ t("settings.apiKeyClear") }}
          </button>
        </div>
        <p class="muted hint">{{ t("settings.apiKeyHint") }}</p>
        <button
          v-if="apiKeyConfigured"
          type="button"
          :disabled="testing"
          @click="emit('test-geocode')"
        >
          {{ testing ? t("settings.testing") : t("settings.testGeocode") }}
        </button>
        <p v-if="testResult" class="muted">{{ testResult }}</p>
      </div>

      <p v-if="error" class="error">{{ error }}</p>

      <div class="actions">
        <button type="button" @click="emit('close')">{{ t("settings.cancel") }}</button>
        <button type="button" class="primary" @click="save">{{ t("settings.save") }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.dialog {
  width: 560px;
  max-width: 92vw;
  max-height: 86vh;
  overflow-y: auto;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 18px 20px;
}

h2 {
  margin: 0 0 12px;
  font-size: 16px;
}

.field {
  margin-bottom: 12px;
}

.field > label {
  display: block;
  margin-bottom: 4px;
  font-weight: 600;
}

.inline {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

input[type="number"] {
  width: 64px;
}

input,
select {
  padding: 5px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg);
  color: var(--text);
  font: inherit;
}

.checkbox {
  display: flex;
  align-items: center;
  gap: 6px;
  font-weight: 400;
}

.muted {
  color: var(--muted);
  font-size: 12px;
}

.hint {
  margin: 4px 0 0;
}

.error {
  color: var(--error);
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 16px;
}
</style>
