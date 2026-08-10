import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import ProgressPanel from "./ProgressPanel.vue";
import { mountPlugins } from "./test-utils";
import type { ProgressEvent } from "../types/task";

const event: ProgressEvent = {
  jobId: "j1",
  phase: "executing",
  current: 250,
  total: 1000,
  currentFile: "/src/a.jpg",
  success: 230,
  skipped: 5,
  duplicate: 10,
  failed: 5,
  percent: 25,
};

describe("ProgressPanel", () => {
  it("激活时渲染百分比与计数", () => {
    const wrapper = mount(ProgressPanel, {
      props: { progress: event, active: true },
      global: { plugins: mountPlugins() },
    });
    expect(wrapper.text()).toContain("250 / 1000");
    expect(wrapper.text()).toContain("25.0%");
    expect(wrapper.text()).toContain("成功 230");
    expect(wrapper.text()).toContain("重复 10");
    expect(wrapper.text()).toContain("/src/a.jpg");
  });

  it("非激活或空进度时不渲染", () => {
    const idle = mount(ProgressPanel, {
      props: { progress: null, active: true },
      global: { plugins: mountPlugins() },
    });
    expect(idle.find(".progress-panel").exists()).toBe(false);
    const inactive = mount(ProgressPanel, {
      props: { progress: event, active: false },
      global: { plugins: mountPlugins() },
    });
    expect(inactive.find(".progress-panel").exists()).toBe(false);
  });
});
