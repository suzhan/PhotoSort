import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import SettingsDialog from "./SettingsDialog.vue";
import { mountPlugins } from "./test-utils";
import type { AppSettings } from "../types/settings";

function baseSettings(): AppSettings {
  return {
    sourceDirectory: null,
    destinationDirectory: null,
    includeSubfolders: true,
    operationMode: "copyVerifyDelete",
    directoryTemplate: "{yyyy}/{camera_model}",
    filenameTemplate: "{original_name}.{extension}",
    duplicateMode: "modern",
    gpsEnabled: false,
    gpsPathLevel: "city",
    gpsRoundPrecision: 4,
    gpsNoApiMode: "coordinates",
    metadataFallback: {
      useModifiedTime: false,
      unknownCamera: "UnknownCamera",
      unknownLocation: "UnknownLocation",
      unknownDate: "UnknownDate",
    },
    maxHashWorkers: 4,
    maxCopyWorkers: 2,
    theme: "system",
    language: "zh-CN",
  };
}

describe("SettingsDialog", () => {
  it("保存时把副本（含钳制后的并发数）交给父组件", async () => {
    const wrapper = mount(SettingsDialog, {
      props: { open: true, settings: baseSettings(), apiKeyConfigured: false, testing: false, testResult: "" },
      global: { plugins: mountPlugins() },
    });
    const numbers = wrapper.findAll('input[type="number"]');
    await numbers[0].setValue(999); // copyWorkers 超界
    await wrapper.find("button.primary").trigger("click");
    const saved = wrapper.emitted("save");
    expect(saved).toBeTruthy();
    const payload = saved![0][0] as AppSettings;
    expect(payload.maxCopyWorkers).toBe(16);
  });

  it("占位名称为空时拒绝保存", async () => {
    const s = baseSettings();
    s.metadataFallback.unknownCamera = "  ";
    const wrapper = mount(SettingsDialog, {
      props: { open: true, settings: s, apiKeyConfigured: false, testing: false, testResult: "" },
      global: { plugins: mountPlugins() },
    });
    await wrapper.find("button.primary").trigger("click");
    expect(wrapper.emitted("save")).toBeFalsy();
    expect(wrapper.text()).toContain("占位名称不能为空");
  });

  it("不修改传入的 settings 对象（副本语义）", async () => {
    const s = baseSettings();
    const wrapper = mount(SettingsDialog, {
      props: { open: true, settings: s, apiKeyConfigured: false, testing: false, testResult: "" },
      global: { plugins: mountPlugins() },
    });
    const select = wrapper.find("select");
    await select.setValue("legacyStrict");
    expect(s.duplicateMode).toBe("modern");
  });

  it("点取消按钮触发 close 事件", async () => {
    const wrapper = mount(SettingsDialog, {
      props: { open: true, settings: baseSettings(), apiKeyConfigured: false, testing: false, testResult: "" },
      global: { plugins: mountPlugins() },
    });
    const buttons = wrapper.findAll("button");
    const cancel = buttons.find((b) => b.text().includes("取消"));
    expect(cancel).toBeTruthy();
    await cancel!.trigger("click");
    expect(wrapper.emitted("close")).toBeTruthy();
  });
});
