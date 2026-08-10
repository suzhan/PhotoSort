import { defineStore } from "pinia";
import {
  cancelJob,
  onOrganizeProgress,
  organizePhotos,
} from "../services/tauri";
import type { OrganizeSummary, ProgressEvent } from "../types/task";

/**
 * 整理执行状态（Phase 9）。
 * organize_photos 完成时 resolve 汇总；期间 organize-progress 事件驱动实时进度。
 */
export const useTaskStore = defineStore("task", {
  state: () => ({
    jobId: null as string | null,
    organizing: false,
    progress: null as ProgressEvent | null,
    summary: null as OrganizeSummary | null,
    cancelling: false,
    error: null as string | null,
    listenersBound: false,
  }),
  actions: {
    async bindListeners() {
      if (this.listenersBound) return;
      this.listenersBound = true;
      await onOrganizeProgress((event) => {
        if (this.organizing) {
          this.jobId = event.jobId;
          this.progress = event;
        }
      });
    },

    async start() {
      await this.bindListeners();
      this.jobId = null;
      this.organizing = true;
      this.cancelling = false;
      this.progress = null;
      this.summary = null;
      this.error = null;
      try {
        this.summary = await organizePhotos();
        this.jobId = this.summary.jobId;
      } catch (e) {
        this.error = String(e);
      } finally {
        this.organizing = false;
        this.cancelling = false;
      }
    },

    async cancel() {
      if (!this.jobId) return;
      this.cancelling = true;
      // 在途文件完成到安全状态后任务才退出；organizing 由 start 的 finally 收尾
      await cancelJob(this.jobId).catch(() => false);
    },
  },
});
