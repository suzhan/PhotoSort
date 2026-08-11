import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import RuleEditor from "./RuleEditor.vue";
import { mountPlugins } from "./test-utils";

vi.mock("../services/tauri", () => ({
  templatePreview: vi.fn(),
}));

import { templatePreview } from "../services/tauri";
const mockedPreview = vi.mocked(templatePreview);

function mountEditor(attachTo?: Element) {
  return mount(RuleEditor, {
    props: {
      directoryTemplate: "{yyyy}/{camera_model}",
      filenameTemplate: "{original_name}.{extension}",
    },
    global: { plugins: mountPlugins() },
    attachTo,
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
    const input = wrapper.find('[data-test="directory-input"]');
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
    await wrapper.find('[data-test="directory-input"]').setValue("{yyyy");
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
    const input = wrapper.find('[data-test="directory-input"]');
    await input.setValue("{yyyy}");
    await vi.advanceTimersByTimeAsync(100);
    await input.setValue("{yyyy}/{MM}");
    await vi.advanceTimersByTimeAsync(100);
    await input.setValue("{yyyy}/{MM}/{dd}");
    await vi.advanceTimersByTimeAsync(300);
    await flushPromises();
    expect(mockedPreview).toHaveBeenCalledTimes(1);
  });

  it("选择预设会同步目录和文件模板", async () => {
    mockedPreview.mockResolvedValue({
      directoryComponents: ["2017", "Hong Kong", "Nikon D80"],
      filename: "DSC_1231.JPG",
      example: "2017/Hong Kong/Nikon D80/DSC_1231.JPG",
    });
    const wrapper = mountEditor();
    await wrapper.find('[data-test="preset"]').setValue("yearCityCamera");
    expect(wrapper.emitted("update:directoryTemplate")?.at(-1)).toEqual([
      "{yyyy}/{gps_city}/{camera_model}",
    ]);
    expect(wrapper.emitted("update:filenameTemplate")?.at(-1)).toEqual([
      "{original_name}.{extension}",
    ]);
  });

  it("中文界面显示中文字段名但模板代码保持英文", async () => {
    mockedPreview.mockResolvedValue({
      directoryComponents: ["2017", "Nikon D80"],
      filename: "DSC_1231.JPG",
      example: "2017/Nikon D80/DSC_1231.JPG",
    });
    const wrapper = mountEditor();

    expect(wrapper.find('[data-test="directory-builder"]').text()).toContain("年份");
    expect(wrapper.find('[data-test="directory-builder"]').text()).toContain("相机型号");
    expect(wrapper.find('[data-test="directory-builder"]').text()).not.toContain("Year");

    await wrapper.find('[data-test="preset"]').setValue("dateSequence");
    expect(wrapper.emitted("update:directoryTemplate")?.at(-1)).toEqual([
      "{yyyy}/{yyyyMMdd}",
    ]);
    expect(wrapper.emitted("update:filenameTemplate")?.at(-1)).toEqual([
      "{yyyyMMdd}_{HHmmss}_{seq:4}.{extension}",
    ]);
  });

  it("点击添加字段面板外部会关闭面板", async () => {
    mockedPreview.mockResolvedValue({
      directoryComponents: ["2017", "Nikon D80"],
      filename: "DSC_1231.JPG",
      example: "2017/Nikon D80/DSC_1231.JPG",
    });
    const wrapper = mountEditor(document.body);
    await wrapper.find('[data-test="directory-menu"] summary').trigger("click");
    expect((wrapper.find('[data-test="directory-menu"]').element as HTMLDetailsElement).open).toBe(true);

    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await wrapper.vm.$nextTick();

    expect((wrapper.find('[data-test="directory-menu"]').element as HTMLDetailsElement).open).toBe(false);
    wrapper.unmount();
  });

  it("选择字段后保持添加字段面板打开", async () => {
    mockedPreview.mockResolvedValue({
      directoryComponents: ["2017", "Nikon D80", "11"],
      filename: "DSC_1231.JPG",
      example: "2017/Nikon D80/11/DSC_1231.JPG",
    });
    const wrapper = mountEditor(document.body);
    await wrapper.find('[data-test="directory-menu"] summary').trigger("click");
    await wrapper
      .findAll('[data-test="directory-menu"] button')
      .find((button) => button.text() === "月份")
      ?.trigger("click");

    expect((wrapper.find('[data-test="directory-menu"]').element as HTMLDetailsElement).open).toBe(true);
    expect(wrapper.emitted("update:directoryTemplate")?.at(-1)).toEqual([
      "{yyyy}/{camera_model}/{MM}",
    ]);
    wrapper.unmount();
  });
});
