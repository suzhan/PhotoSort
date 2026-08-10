/** 与 Rust db::jobs::PendingJobSummary 一致。 */
export interface PendingJobSummary {
  jobId: string;
  kind: "scan" | "organize";
  sourceRoot: string | null;
  destinationRoot: string | null;
  startedAt: number;
  totalFiles: number;
  finishedFiles: number;
}
