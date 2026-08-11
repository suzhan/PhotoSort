<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
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

type FieldScope = "directory" | "filename";
type TemplatePart = {
  value: string;
  label: string;
  field: boolean;
};
type RuleField = {
  key: string;
  template: string;
  labelKey: string;
  category: string;
  scopes: FieldScope[];
};

const { t } = useI18n();
const preview = ref<TemplatePreview | null>(null);
const previewError = ref("");
const advancedOpen = ref(false);
const openFieldMenu = ref<FieldScope | null>(null);
const directoryMenu = ref<HTMLElement | null>(null);
const filenameMenu = ref<HTMLElement | null>(null);

const dirLocal = ref(props.directoryTemplate);
const fileLocal = ref(props.filenameTemplate);
watch(() => props.directoryTemplate, (v) => (dirLocal.value = v));
watch(() => props.filenameTemplate, (v) => (fileLocal.value = v));

const presets = [
  {
    key: "yearCameraOriginal",
    dir: "{yyyy}/{camera_model}",
    file: "{original_name}.{extension}",
  },
  {
    key: "yearDateCamera",
    dir: "{yyyy}/{yyyyMMdd}/{camera_model}",
    file: "{original_name}.{extension}",
  },
  {
    key: "yearCityCamera",
    dir: "{yyyy}/{gps_city}/{camera_model}",
    file: "{original_name}.{extension}",
  },
  {
    key: "dateSequence",
    dir: "{yyyy}/{yyyyMMdd}",
    file: "{yyyyMMdd}_{HHmmss}_{seq:4}.{extension}",
  },
];

const fields: RuleField[] = [
  { key: "yyyy", template: "{yyyy}", labelKey: "year", category: "date", scopes: ["directory", "filename"] },
  { key: "MM", template: "{MM}", labelKey: "month", category: "date", scopes: ["directory", "filename"] },
  { key: "dd", template: "{dd}", labelKey: "day", category: "date", scopes: ["directory", "filename"] },
  { key: "yyyyMMdd", template: "{yyyyMMdd}", labelKey: "fullDate", category: "date", scopes: ["directory", "filename"] },
  { key: "HHmmss", template: "{HHmmss}", labelKey: "time", category: "date", scopes: ["filename"] },
  { key: "camera_make", template: "{camera_make}", labelKey: "cameraMake", category: "camera", scopes: ["directory", "filename"] },
  { key: "camera_model", template: "{camera_model}", labelKey: "cameraModel", category: "camera", scopes: ["directory", "filename"] },
  { key: "lens_model", template: "{lens_model}", labelKey: "lensModel", category: "camera", scopes: ["directory", "filename"] },
  { key: "gps_country", template: "{gps_country}", labelKey: "country", category: "location", scopes: ["directory", "filename"] },
  { key: "gps_province", template: "{gps_province}", labelKey: "province", category: "location", scopes: ["directory", "filename"] },
  { key: "gps_city", template: "{gps_city}", labelKey: "city", category: "location", scopes: ["directory", "filename"] },
  { key: "gps_district", template: "{gps_district}", labelKey: "district", category: "location", scopes: ["directory", "filename"] },
  { key: "original_name", template: "{original_name}", labelKey: "originalName", category: "file", scopes: ["filename"] },
  { key: "extension", template: "{extension}", labelKey: "extension", category: "file", scopes: ["filename"] },
  { key: "seq4", template: "{seq:4}", labelKey: "sequence", category: "file", scopes: ["filename"] },
];

const fieldByTemplate = new Map(fields.map((field) => [field.template, field]));
const tokenPattern = /(\{[^}]+\}|[\/._ -])/g;

const selectedPreset = computed(() => {
  const match = presets.find((preset) => preset.dir === dirLocal.value && preset.file === fileLocal.value);
  return match?.key ?? "custom";
});

const directoryParts = computed(() => parseTemplate(dirLocal.value));
const filenameParts = computed(() => parseTemplate(fileLocal.value));

let timer: ReturnType<typeof setTimeout> | undefined;

function parseTemplate(template: string): TemplatePart[] {
  return template
    .split(tokenPattern)
    .filter(Boolean)
    .map((value) => {
      const field = fieldByTemplate.get(value);
      return {
        value,
        label: field ? fieldLabel(field) : value,
        field: Boolean(field),
      };
    });
}

function fieldLabel(field: RuleField): string {
  return t(`rules.fields.${field.labelKey}`);
}

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
    const raw = e as { message?: string; key?: string };
    previewError.value = raw?.message ?? String(e);
  }
}

function syncTemplates(directoryTemplate = dirLocal.value, filenameTemplate = fileLocal.value) {
  dirLocal.value = directoryTemplate;
  fileLocal.value = filenameTemplate;
  emit("update:directoryTemplate", dirLocal.value);
  emit("update:filenameTemplate", fileLocal.value);
  schedulePreview();
}

function onPreset(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  const preset = presets.find((item) => item.key === value);
  if (preset) {
    syncTemplates(preset.dir, preset.file);
  }
}

function appendToken(scope: FieldScope, token: string) {
  if (scope === "directory") {
    const separator = dirLocal.value && !dirLocal.value.endsWith("/") ? "/" : "";
    syncTemplates(`${dirLocal.value}${separator}${token}`, fileLocal.value);
    return;
  }

  const separator = token === "{extension}" ? (fileLocal.value.endsWith(".") ? "" : ".") : fileLocal.value && !/[._ -]$/.test(fileLocal.value) ? "_" : "";
  syncTemplates(dirLocal.value, `${fileLocal.value}${separator}${token}`);
}

function removePart(scope: FieldScope, index: number) {
  const source = scope === "directory" ? directoryParts.value : filenameParts.value;
  const next = source
    .filter((_, i) => i !== index)
    .map((part) => part.value)
    .join("")
    .replace(/\/{2,}/g, "/")
    .replace(/^[\/._ -]+|[\/._ -]+$/g, "");
  if (scope === "directory") {
    syncTemplates(next, fileLocal.value);
  } else {
    syncTemplates(dirLocal.value, next);
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

function fieldsFor(scope: FieldScope, category: string) {
  return fields.filter((field) => field.category === category && field.scopes.includes(scope));
}

function onFieldMenuToggle(scope: FieldScope, event: Event) {
  const isOpen = (event.target as HTMLDetailsElement).open;
  openFieldMenu.value = isOpen ? scope : openFieldMenu.value === scope ? null : openFieldMenu.value;
}

function onDocumentPointerDown(event: PointerEvent) {
  const target = event.target as Node;
  if (directoryMenu.value?.contains(target) || filenameMenu.value?.contains(target)) {
    return;
  }
  directoryMenu.value?.removeAttribute("open");
  filenameMenu.value?.removeAttribute("open");
  openFieldMenu.value = null;
}

onMounted(() => {
  document.addEventListener("pointerdown", onDocumentPointerDown);
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocumentPointerDown);
});

defineExpose({ refresh });
</script>

<template>
  <div class="rule-editor">
    <div class="preset-row">
      <label class="field-label" for="rule-preset">{{ t("rules.preset") }}</label>
      <select id="rule-preset" class="preset-select" :value="selectedPreset" data-test="preset" @change="onPreset">
        <option v-for="preset in presets" :key="preset.key" :value="preset.key">
          {{ t(`rules.presets.${preset.key}`) }}
        </option>
        <option value="custom">{{ t("rules.presets.custom") }}</option>
      </select>
    </div>

    <div class="builder-grid">
      <section class="builder-section" aria-labelledby="folder-rule-title">
        <div class="builder-heading">
          <span id="folder-rule-title">{{ t("rules.folderStructure") }}</span>
          <details
            ref="directoryMenu"
            class="field-menu"
            :open="openFieldMenu === 'directory'"
            data-test="directory-menu"
            @toggle="onFieldMenuToggle('directory', $event)"
          >
            <summary>{{ t("rules.addField") }}</summary>
            <div class="field-palette">
              <div v-for="category in ['date', 'camera', 'location']" :key="category" class="field-group">
                <span class="field-group-title">{{ t(`rules.categories.${category}`) }}</span>
                <button
                  v-for="field in fieldsFor('directory', category)"
                  :key="field.key"
                  type="button"
                  @click="appendToken('directory', field.template)"
                >
                  {{ fieldLabel(field) }}
                </button>
              </div>
            </div>
          </details>
        </div>
        <div class="chip-row" data-test="directory-builder">
          <button
            v-for="(part, index) in directoryParts"
            :key="`${part.value}-${index}`"
            type="button"
            class="chip"
            :class="{ field: part.field, literal: !part.field }"
            :title="part.field ? t('rules.removeField') : part.value"
            @click="part.field && removePart('directory', index)"
          >
            {{ part.label }}
          </button>
        </div>
      </section>

      <section class="builder-section" aria-labelledby="filename-rule-title">
        <div class="builder-heading">
          <span id="filename-rule-title">{{ t("rules.filename") }}</span>
          <details
            ref="filenameMenu"
            class="field-menu"
            :open="openFieldMenu === 'filename'"
            data-test="filename-menu"
            @toggle="onFieldMenuToggle('filename', $event)"
          >
            <summary>{{ t("rules.addField") }}</summary>
            <div class="field-palette">
              <div v-for="category in ['date', 'camera', 'location', 'file']" :key="category" class="field-group">
                <span class="field-group-title">{{ t(`rules.categories.${category}`) }}</span>
                <button
                  v-for="field in fieldsFor('filename', category)"
                  :key="field.key"
                  type="button"
                  @click="appendToken('filename', field.template)"
                >
                  {{ fieldLabel(field) }}
                </button>
              </div>
            </div>
          </details>
        </div>
        <div class="chip-row" data-test="filename-builder">
          <button
            v-for="(part, index) in filenameParts"
            :key="`${part.value}-${index}`"
            type="button"
            class="chip"
            :class="{ field: part.field, literal: !part.field }"
            :title="part.field ? t('rules.removeField') : part.value"
            @click="part.field && removePart('filename', index)"
          >
            {{ part.label }}
          </button>
        </div>
      </section>
    </div>

    <div v-if="preview" class="preview-box">
      <div>
        <span>{{ t("rules.previewFolder") }}</span>
        <strong class="mono">{{ preview.directoryComponents.join("/") || "." }}</strong>
      </div>
      <div>
        <span>{{ t("rules.previewFilename") }}</span>
        <strong class="mono">{{ preview.filename }}</strong>
      </div>
      <div>
        <span>{{ t("rules.previewFullPath") }}</span>
        <strong class="mono">{{ preview.example }}</strong>
      </div>
    </div>
    <p v-if="previewError" class="error">
      {{ t("rules.invalid") }}: {{ previewError }}
    </p>

    <details class="advanced" :open="advancedOpen" @toggle="advancedOpen = ($event.target as HTMLDetailsElement).open">
      <summary>{{ t("rules.advanced") }}</summary>
      <div class="row">
        <label class="field-label">{{ t("rules.directoryTemplate") }}</label>
        <input
          class="path-input mono"
          :value="dirLocal"
          data-test="directory-input"
          placeholder="{yyyy}/{camera_model}"
          @input="onDir"
        />
      </div>
      <div class="row">
        <label class="field-label">{{ t("rules.filenameTemplate") }}</label>
        <input
          class="path-input mono"
          :value="fileLocal"
          data-test="filename-input"
          placeholder="{yyyyMMdd}_{HHmmss}_{seq:4}.{extension}"
          @input="onFile"
        />
      </div>
    </details>
  </div>
</template>

<style scoped>
.rule-editor {
  display: grid;
  gap: 12px;
}

.preset-row,
.row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.field-label {
  min-width: 112px;
  color: var(--muted);
}

.preset-select,
.path-input {
  flex: 1;
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg);
  color: var(--text);
}

.builder-grid {
  display: grid;
  gap: 10px;
}

.builder-section {
  display: grid;
  gap: 8px;
}

.builder-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-weight: 600;
}

.field-menu {
  position: relative;
}

.field-menu summary {
  list-style: none;
  cursor: pointer;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 4px 10px;
  background: var(--panel);
  font-weight: 500;
}

.field-menu summary::-webkit-details-marker {
  display: none;
}

.field-palette {
  position: absolute;
  right: 0;
  z-index: 2;
  display: grid;
  width: min(460px, 78vw);
  gap: 10px;
  margin-top: 6px;
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--panel);
  box-shadow: 0 12px 28px rgb(0 0 0 / 12%);
}

.field-group {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.field-group-title {
  width: 100%;
  color: var(--muted);
  font-size: 12px;
  font-weight: 600;
}

.chip-row {
  display: flex;
  min-height: 38px;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  padding: 6px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg);
}

.chip {
  min-height: 26px;
  padding: 3px 8px;
  border-radius: 6px;
  font-size: 13px;
}

.chip.field {
  border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
  background: color-mix(in srgb, var(--accent) 12%, var(--panel));
  color: var(--text);
}

.chip.literal {
  cursor: default;
  color: var(--muted);
}

.preview-box {
  display: grid;
  gap: 6px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: color-mix(in srgb, var(--ok) 8%, var(--panel));
}

.preview-box div {
  display: grid;
  grid-template-columns: 92px minmax(0, 1fr);
  gap: 8px;
}

.preview-box span {
  color: var(--muted);
}

.preview-box strong {
  min-width: 0;
  word-break: break-all;
}

.advanced {
  border-top: 1px solid var(--border);
  padding-top: 8px;
}

.advanced summary {
  cursor: pointer;
  color: var(--muted);
}

.advanced .row {
  margin-top: 8px;
}

.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}

.error {
  color: var(--error);
  word-break: break-all;
}

@media (max-width: 720px) {
  .preset-row,
  .row,
  .preview-box div {
    grid-template-columns: 1fr;
  }

  .preset-row,
  .row {
    display: grid;
  }

  .field-label {
    min-width: 0;
  }
}
</style>
