/** 扫描结果分页行。字段与 Rust 侧 ScanRowDto / PlanRow DTO 一一对应。 */
export interface ScanRow {
  seq: number;
  sourcePath: string;
  size: number | null;
  takenAt: string | null;
  camera: string | null;
  lens: string | null;
  gps: string | null;
  targetPath: string | null;
  status: string;
  warning: string | null;
}

/** scan-items 事件载荷。 */
export interface ScanItemsBatch {
  jobId: string;
  rows: ScanRow[];
}

/** scan_photos 命令返回值。 */
export interface ScanSummary {
  jobId: string;
  root: string;
  found: number;
  skippedHidden: number;
  skippedUnsupported: number;
  skippedNonFile: number;
  errors: number;
  cancelled: boolean;
  /** 解析成功但相机/时间/GPS 全缺。 */
  metadataMissing: number;
  /** 所有可用引擎均解析失败。 */
  metadataFailed: number;
  /** 已生成 Plan 的数量（设置了目标目录才 > 0）。 */
  planned: number;
  /** 目标路径被占用且无法自动避让的数量。 */
  collisions: number;
  /** 内容核实为完全重复的数量。 */
  duplicates: number;
  /** 规划失败的数量。 */
  planErrors: number;
}
