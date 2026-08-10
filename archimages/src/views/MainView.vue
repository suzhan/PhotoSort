<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useSettingsStore } from "../stores/settings";
import { useScanStore } from "../stores/scan";
import { useTaskStore } from "../stores/task";
import { useLogStore } from "../stores/log";
import {
  abandonJob,
  clearGoogleApiKey,
  hasGoogleApiKey,
  pendingRecoveryJobs,
  setGoogleApiKey,
  testGeocode,
} from "../services/tauri";
import type { AppLocale, AppSettings, OperationMode } from "../types/settings";
import type { PendingJobSummary } from "../types/job";
import DirectoryPicker from "../components/DirectoryPicker.vue";
import OperationModePicker from "../components/OperationModePicker.vue";
import RuleEditor from "../components/RuleEditor.vue";
import ScanResultTable from "../components/ScanResultTable.vue";
import ProgressPanel from "../components/ProgressPanel.vue";
import LogPanel from "../components/LogPanel.vue";
import SettingsDialog from "../components/SettingsDialog.vue";
import RecoveryBanner from "../components/RecoveryBanner.vue";

const { t, locale } = useI18n();
const settings = useSettingsStore();
const scan = useScanStore();
const task = useTaskStore();
const logs = useLogStore();

const sourceInput = ref("");
const destInput = ref("");
const includeSubfolders = ref(true);
const dirTemplate = ref("");
const fileTemplate = ref("");
const ruleEditor = ref<InstanceType<typeof RuleEditor> | null>(null);
const pendingJobs = ref<PendingJobSummary[]>([]);
const settingsOpen = ref(false);
const apiKeyConfigured = ref(false);
const geocodeTestResult = ref("");
const geocodeTesting = ref(false);

/** OperationModePicker 直接绑到 settings，persistDraft 统一落盘。 */
const operationModeProxy = computed<OperationMode>({
  get: () => settings.settings?.operationMode ?? "copyVerifyDelete",
  set: (v) => {
    if (settings.settings) settings.settings.operationMode = v;
  },
});

onMounted(async () => {
  try {
    await settings.load();
    locale.value = settings.locale;
    sourceInput.value = settings.settings?.sourceDirectory ?? "";
    destInput.value = settings.settings?.destinationDirectory ?? "";
    includeSubfolders.value = settings.settings?.includeSubfolders ?? true;
    dirTemplate.value = settings.settings?.directoryTemplate ?? "";
    fileTemplate.value = settings.settings?.filenameTemplate ?? "";
    ruleEditor.value?.refresh();
  } catch (e) {
    logs.push(`加载设置失败：${String(e)}`);
  }
  try {
    pendingJobs.value = await pendingRecoveryJobs();
  } catch (e) {
    logs.push(`查询未完成任务失败：${String(e)}`);
  }
  try {
    apiKeyConfigured.value = await hasGoogleApiKey();
  } catch {
    // keyring 不可用时静默
  }
});

// 主题：system 时不设属性，交给 prefers-color-scheme
watch(
  () => settings.settings?.theme,
  (theme) => {
    const el = document.documentElement;
    if (theme === "light" || theme === "dark") {
      el.dataset.theme = theme;
    } else {
      delete el.dataset.theme;
    }
  },
  { immediate: true },
);

/** 扫描/整理前把当前 UI 现场持久化（后端命令读 settings 快照）。 */
async function persistDraft(): Promise<boolean> {
  if (!settings.settings) return false;
  settings.settings.sourceDirectory = sourceInput.value.trim() || null;
  settings.settings.destinationDirectory = destInput.value.trim() || null;
  settings.settings.includeSubfolders = includeSubfolders.value;
  settings.settings.directoryTemplate = dirTemplate.value;
  settings.settings.filenameTemplate = fileTemplate.value;
  try {
    await settings.persist();
    return true;
  } catch (e) {
    logs.push(`保存设置失败：${String(e)}`);
    return false;
  }
}

async function startScan() {
  const root = sourceInput.value.trim();
  if (!root) return;
  if (!(await persistDraft())) return;
  await scan.start(root, includeSubfolders.value);
}

async function startOrganize() {
  if (!(await persistDraft())) return;
  await task.start();
}

async function onAbandon(jobId: string) {
  try {
    await abandonJob(jobId);
    pendingJobs.value = pendingJobs.value.filter((j) => j.jobId !== jobId);
  } catch (e) {
    logs.push(`放弃任务失败：${String(e)}`);
  }
}

async function onSaveSettings(next: AppSettings) {
  try {
    settings.settings = next;
    await settings.persist();
    settingsOpen.value = false;
  } catch (e) {
    logs.push(`保存设置失败：${String(e)}`);
  }
}

async function saveApiKey(key: string) {
  const k = key.trim();
  if (!k) return;
  try {
    await setGoogleApiKey(k);
    apiKeyConfigured.value = true;
    logs.push(t("settings.apiKeySaved"));
  } catch (e) {
    logs.push(`${t("settings.apiKeySaveFailed")}：${String(e)}`);
  }
}

async function removeApiKey() {
  try {
    await clearGoogleApiKey();
    apiKeyConfigured.value = false;
    logs.push(t("settings.apiKeyCleared"));
  } catch (e) {
    logs.push(`${t("settings.apiKeyClearFailed")}：${String(e)}`);
  }
}

async function runGeocodeTest() {
  geocodeTesting.value = true;
  geocodeTestResult.value = "";
  try {
    const addr = await testGeocode(22.3193, 114.1694);
    geocodeTestResult.value = addr;
  } catch (e) {
    geocodeTestResult.value = String(e);
  } finally {
    geocodeTesting.value = false;
  }
}

function switchLocale(value: AppLocale) {
  settings.setLocale(value);
  locale.value = value;
  settings.persist().catch((e) => logs.push(`保存设置失败：${String(e)}`));
}
</script>

<template>
  <main class="container">
    <header class="app-header">
      <div>
        <h1>{{ t("app.title") }}</h1>
        <span class="subtitle">{{ t("app.subtitle") }}</span>
      </div>
      <div class="header-actions">
        <button
          :class="{ active: settings.locale === 'zh-CN' }"
          @click="switchLocale('zh-CN')"
        >
          {{ t("language.zhCN") }}
        </button>
        <button
          :class="{ active: settings.locale === 'en' }"
          @click="switchLocale('en')"
        >
          {{ t("language.en") }}
        </button>
        <button @click="settingsOpen = true">{{ t("settings.open") }}</button>
      </div>
    </header>

    <RecoveryBanner :jobs="pendingJobs" @abandon="onAbandon" />

    <section class="panel">
      <DirectoryPicker
        v-model="sourceInput"
        :label="t('scan.sourceLabel')"
        :placeholder="t('scan.sourcePlaceholder')"
      />
      <DirectoryPicker
        v-model="destInput"
        :label="t('scan.destLabel')"
        :placeholder="t('scan.destPlaceholder')"
      />
      <div class="row">
        <label class="checkbox">
          <input type="checkbox" v-model="includeSubfolders" />
          {{ t("scan.includeSubfolders") }}
        </label>
      </div>
    </section>

    <section class="panel">
      <label class="section-label">{{ t("mode.title") }}</label>
      <OperationModePicker v-model="operationModeProxy" />
    </section>

    <section class="panel">
      <label class="section-label">{{ t("rules.title") }}</label>
      <RuleEditor
        ref="ruleEditor"
        :directory-template="dirTemplate"
        :filename-template="fileTemplate"
        @update:directory-template="dirTemplate = $event"
        @update:filename-template="fileTemplate = $event"
      />
    </section>

    <section class="panel">
      <div class="row actions-row">
        <button
          class="primary"
          :disabled="scan.scanning || task.organizing || !sourceInput.trim()"
          @click="startScan"
        >
          {{ scan.scanning ? t("scan.scanning", { count: scan.found }) : t("scan.start") }}
        </button>
        <button
          class="primary"
          :disabled="
            task.organizing ||
            scan.scanning ||
            !sourceInput.trim() ||
            !destInput.trim()
          "
          @click="startOrganize"
        >
          {{ task.organizing ? t("organize.running") : t("organize.start") }}
        </button>
        <button
          v-if="task.organizing"
          :disabled="task.cancelling"
          @click="task.cancel()"
        >
          {{ task.cancelling ? t("organize.cancelling") : t("organize.cancel") }}
        </button>
      </div>

      <ProgressPanel :progress="task.progress" :active="task.organizing" />

      <template v-if="task.summary">
        <p class="muted">
          {{
            t("organize.report", {
              total: task.summary.total,
              success: task.summary.success,
              duplicate: task.summary.duplicate,
              skipped: task.summary.skipped,
              failed: task.summary.failed,
            })
          }}{{ task.summary.cancelled ? t("organize.cancelled") : "" }}
        </p>
        <ul v-if="task.summary.errors.length" class="errors">
          <li v-for="err in task.summary.errors" :key="err.source">
            {{ err.source }} — {{ err.message }}
          </li>
        </ul>
      </template>
      <p v-if="task.error" class="error">{{ task.error }}</p>

      <template v-if="scan.summary">
        <p class="muted">
          {{
            t("scan.summary", {
              found: scan.summary.found,
              hidden: scan.summary.skippedHidden,
              unsupported: scan.summary.skippedUnsupported,
              errors: scan.summary.errors,
            })
          }}{{ scan.summary.cancelled ? t("scan.summaryCancelled") : "" }}
          <template v-if="scan.summary.planned > 0">
            {{
              t("scan.summaryPlanned", {
                planned: scan.summary.planned,
                duplicates: scan.summary.duplicates,
                collisions: scan.summary.collisions,
                planErrors: scan.summary.planErrors,
              })
            }}
          </template>
        </p>
      </template>
      <p v-if="scan.error" class="error">{{ scan.error }}</p>

      <ScanResultTable v-if="scan.rows.length" :rows="scan.rows" />
    </section>

    <section class="panel">
      <LogPanel />
    </section>

    <SettingsDialog
      :open="settingsOpen"
      :settings="settings.settings"
      :api-key-configured="apiKeyConfigured"
      :testing="geocodeTesting"
      :test-result="geocodeTestResult"
      @close="settingsOpen = false"
      @save="onSaveSettings"
      @save-key="saveApiKey"
      @clear-key="removeApiKey"
      @test-geocode="runGeocodeTest"
    />
  </main>
</template>

<style scoped>
.container {
  max-width: 900px;
  margin: 0 auto;
  padding: 24px 20px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.app-header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}

.app-header h1 {
  margin: 0;
  font-size: 22px;
  display: inline;
}

.subtitle {
  color: var(--muted);
  margin-left: 8px;
}

.header-actions {
  display: flex;
  gap: 8px;
}

.panel {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 14px 16px;
}

.panel p {
  margin: 4px 0;
}

.row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 8px;
}

.actions-row {
  margin-top: 0;
}

.section-label {
  font-weight: 600;
}

.checkbox {
  display: flex;
  align-items: center;
  gap: 6px;
}

.muted {
  color: var(--muted);
}

.error {
  color: var(--error);
  word-break: break-all;
}

.errors {
  max-height: 120px;
  overflow: auto;
  margin: 6px 0 0;
  padding-left: 18px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  word-break: break-all;
  color: var(--error);
}

button.active {
  border-color: var(--accent);
  color: var(--accent);
}
</style>
