import { defineStore } from "pinia";
import { getSettings, saveSettings } from "../services/tauri";
import type { AppLocale, AppSettings } from "../types/settings";

export const useSettingsStore = defineStore("settings", {
  state: () => ({
    locale: "zh-CN" as AppLocale,
    settings: null as AppSettings | null,
    loaded: false,
  }),
  actions: {
    /** 启动时从后端加载持久化设置。 */
    async load() {
      const settings = await getSettings();
      this.settings = settings;
      this.locale = settings.language === "en" ? "en" : "zh-CN";
      this.loaded = true;
    },
    setLocale(locale: AppLocale) {
      this.locale = locale;
    },
    /** 把当前内存中的设置（含语言）写回后端并持久化。 */
    async persist() {
      if (!this.settings) return;
      this.settings.language = this.locale;
      await saveSettings(this.settings);
    },
  },
});
