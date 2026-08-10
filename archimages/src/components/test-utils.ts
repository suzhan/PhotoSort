import { createI18n } from "vue-i18n";
import { createPinia } from "pinia";
import zhCN from "../i18n/locales/zh-CN";

/** 组件测试统一挂载环境：真实中文语言包 + 独立 Pinia。 */
export function mountPlugins() {
  const i18n = createI18n({
    legacy: false,
    locale: "zh-CN",
    messages: { "zh-CN": zhCN },
  });
  return [i18n, createPinia()];
}
