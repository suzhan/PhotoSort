/** 进度事件。字段与 Rust 侧 ProgressEvent DTO 一一对应（Phase 9 起生效）。 */
export interface ProgressEvent {
  jobId: string;
  phase: string;
  current: number;
  total: number;
  currentFile: string | null;
  success: number;
  skipped: number;
  duplicate: number;
  failed: number;
  percent: number;
}

/** 与 Rust core::organizer::FileError 一致。 */
export interface OrganizeFileError {
  source: string;
  key: string;
  message: string;
}

/** 与 Rust OrganizeSummaryDto（jobId + OrganizeReport flatten）一致。 */
export interface OrganizeSummary {
  jobId: string;
  total: number;
  success: number;
  duplicate: number;
  skipped: number;
  failed: number;
  cancelled: boolean;
  errors: OrganizeFileError[];
}
