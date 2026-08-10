import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import RuleEditor from "./RuleEditor.vue";
import { mountPlugins } from "./test-utils";

vi.mock("../services/tauri", () => ({
  templatePreview: vi.fn(),
}));

import { templatePreview } from "../services/tauri";
const mockedPreview = vi.mocked(templatePreview);

function mountEditor() {
  return mount(RuleEditor, {
    props: {
      directoryTemplate: "{yyyy}/{camera_model}",
      filenameTemplate: "{original_name}.{extension}",
    },
    global: { plugins: mountPlugins() },
  });
}

describe("RuleEditor", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("输入防抖 300ms 后请求预览并展示示例", async () => {
    mockedPreview.mockResolvedValue({
      directoryComponents: ["2017", "Nikon D80"],
      filename: "DSC_1231.JPG",
      example: "2017/Nikon D80/DSC_1231.JPG",
    });
    const wrapper = mountEditor();
    const input = wrapper.find("input");
    await input.setValue("{yyyy}/{gps_city}");
    expect(mockedPreview).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(300);
    await flushPromises();
    expect(mockedPreview).toHaveBeenCalledWith({
      directoryTemplate: "{yyyy}/{gps_city}",
      filenameTemplate: "{original_name}.{extension}",
    });
    expect(wrapper.text()).toContain("2017/Nikon D80/DSC_1231.JPG");
  });

  it("模板非法时展示错误而不是示例", async () => {
    mockedPreview.mockRejectedValue(new Error("unexpected token"));
    const wrapper = mountEditor();
    await wrapper.find("input").setValue("{yyyy");
    await vi.advanceTimersByTimeAsync(300);
    await flushPromises();
    expect(wrapper.text()).toContain("模板无效");
    expect(wrapper.text()).toContain("unexpected token");
  });

  it("连续输入只触发一次 IPC（防抖合并）", async () => {
    mockedPreview.mockResolvedValue({
      directoryComponents: ["x"],
      filename: "x",
      example: "x",
    });
    const wrapper = mountEditor();
    const input = wrapper.find("input");
    await input.setValue("{yyyy}");
    await vi.advanceTimersByTimeAsync(100);
    await input.setValue("{yyyy}/{MM}");
    await vi.advanceTimersByTimeAsync(100);
    await input.setValue("{yyyy}/{MM}/{dd}");
    await vi.advanceTimersByTimeAsync(300);
    await flushPromises();
    expect(mockedPreview).toHaveBeenCalledTimes(1);
  });
});
