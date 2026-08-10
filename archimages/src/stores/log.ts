import { defineStore } from "pinia";

/** UI 日志缓冲有硬上限，避免长任务日志撑爆内存。 */
const MAX_LINES = 500;

export const useLogStore = defineStore("log", {
  state: () => ({
    lines: [] as string[],
  }),
  actions: {
    push(line: string) {
      this.lines.push(line);
      if (this.lines.length > MAX_LINES) {
        this.lines.splice(0, this.lines.length - MAX_LINES);
      }
    },
    clear() {
      this.lines = [];
    },
  },
});
