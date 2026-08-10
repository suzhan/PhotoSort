import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import ScanResultTable from "./ScanResultTable.vue";
import { mountPlugins } from "./test-utils";
import type { ScanRow } from "../types/scan";

function makeRows(count: number): ScanRow[] {
  return Array.from({ length: count }, (_, i) => ({
    seq: i,
    sourcePath: `/src/photo_${i}.jpg`,
    size: 1024,
    takenAt: "2017-11-30 15:22:31",
    camera: "Nikon D80",
    lens: "18-135mm",
    gps: null,
    targetPath: `/dst/2017/photo_${i}.jpg`,
    status:
      i % 7 === 0 ? "duplicate" : i % 11 === 0 ? "missingDate" : "ready",
    warning: null,
  }));
}

describe("ScanResultTable", () => {
  it("一万行只渲染可视窗口内的 DOM 节点", () => {
    const wrapper = mount(ScanResultTable, {
      props: { rows: makeRows(10_000) },
      global: { plugins: mountPlugins() },
    });
    const rendered = wrapper.findAll(".trow");
    expect(rendered.length).toBeLessThan(30);
    expect(rendered.length).toBeGreaterThan(5);
  });

  it("状态列本地化渲染", () => {
    const rows = makeRows(3);
    rows[0].status = "duplicate";
    rows[1].status = "missingDate";
    rows[2].status = "ready";
    const wrapper = mount(ScanResultTable, {
      props: { rows },
      global: { plugins: mountPlugins() },
    });
    const statuses = wrapper.findAll(".col-status").map((n) => n.text());
    expect(statuses).toContain("重复");
    expect(statuses).toContain("缺日期");
    expect(statuses).toContain("就绪");
  });

  it("滚动后渲染窗口跟随", async () => {
    const wrapper = mount(ScanResultTable, {
      props: { rows: makeRows(1000) },
      global: { plugins: mountPlugins() },
      attachTo: document.body,
    });
    const viewport = wrapper.find(".viewport");
    const el = viewport.element as HTMLElement;
    el.scrollTop = 28 * 500; // 跳到第 500 行附近
    await viewport.trigger("scroll");
    expect(wrapper.text()).toContain("photo_500.jpg");
    expect(wrapper.text()).not.toContain("photo_0.jpg");
  });
});
