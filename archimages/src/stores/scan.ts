import { defineStore } from "pinia";
import {
  onScanItems,
  onScanProgress,
  scanPhotos,
} from "../services/tauri";
import type { ScanRow, ScanSummary } from "../types/scan";

/**
 * 扫描状态。rows 保存全量结果（10 万行 ≈ 几十 MB 轻量对象），
 * DOM 渲染压力由 ScanResultTable 的虚拟滚动承担——绝不整表进 DOM。
 */
export const useScanStore = defineStore("scan", {
  state: () => ({
    jobId: null as string | null,
    scanning: false,
    /** 实时已发现数量（来自进度事件）。 */
    found: 0,
    summary: null as ScanSummary | null,
    rows: [] as ScanRow[],
    error: null as string | null,
    listenersBound: false,
  }),
  actions: {
    async bindListeners() {
      if (this.listenersBound) return;
      this.listenersBound = true;
      await onScanProgress((event) => {
        // 同一时刻只允许一个扫描（UI 层保证），进行中即接受。
        if (this.scanning) {
          this.jobId = event.jobId;
          this.found = event.current;
        }
      });
      await onScanItems((batch) => {
        if (this.scanning) {
          this.rows.push(...batch.rows);
        }
      });
    },

    async start(root: string, includeSubfolders: boolean) {
      await this.bindListeners();
      this.jobId = null;
      this.scanning = true;
      this.found = 0;
      this.summary = null;
      this.rows = [];
      this.error = null;
      try {
        const summary = await scanPhotos({ root, includeSubfolders });
        this.jobId = summary.jobId;
        this.summary = summary;
        this.found = summary.found;
      } catch (e) {
        this.error = String(e);
      } finally {
        this.scanning = false;
      }
    },
  },
});
