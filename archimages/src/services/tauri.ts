import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { AppSettings } from "../types/settings";
import type { PendingJobSummary } from "../types/job";
import type { ScanItemsBatch, ScanSummary } from "../types/scan";
import type { OrganizeSummary, ProgressEvent } from "../types/task";
import type { TemplatePreview, TemplatePreviewRequest } from "../types/template";

/**
 * 前端唯一的 IPC 出入口：所有 Tauri command / event 在此登记封装，
 * 组件与 store 不直接调用 invoke / listen。
 */
export async function ping(): Promise<string> {
  return invoke<string>("ping");
}

export async function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  return invoke<void>("save_settings", { settings });
}

/** 弹出系统目录选择器；用户取消时返回 null。 */
export async function pickDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export interface ScanRequestDto {
  root: string;
  includeSubfolders: boolean;
}

export async function scanPhotos(request: ScanRequestDto): Promise<ScanSummary> {
  return invoke<ScanSummary>("scan_photos", { request });
}

/** 订阅扫描进度事件，返回取消订阅函数。 */
export function onScanProgress(
  handler: (event: ProgressEvent) => void,
): Promise<() => void> {
  return listen<ProgressEvent>("scan-progress", (e) => handler(e.payload));
}

/** 订阅扫描行批次事件，返回取消订阅函数。 */
export function onScanItems(
  handler: (batch: ScanItemsBatch) => void,
): Promise<() => void> {
  return listen<ScanItemsBatch>("scan-items", (e) => handler(e.payload));
}

/** 模板实时预览：固定示例上下文渲染目录/文件名模板。 */
export async function templatePreview(
  request: TemplatePreviewRequest,
): Promise<TemplatePreview> {
  if (!isTauriRuntime()) {
    return localTemplatePreview(request);
  }
  return invoke<TemplatePreview>("template_preview", { request });
}

function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window || "__TAURI__" in window;
}

const sampleValues: Record<string, string> = {
  yyyy: "2017",
  MM: "11",
  dd: "30",
  yyyyMMdd: "20171130",
  "yyyy-MM-dd": "2017-11-30",
  HH: "15",
  mm: "22",
  ss: "31",
  HHmmss: "152231",
  camera_make: "NIKON CORPORATION",
  camera_model: "NIKON D80",
  lens_make: "NIKON",
  lens_model: "18-135mm F3.5-5.6",
  gps_country: "China",
  gps_province: "UnknownLocation",
  gps_city: "Hong Kong",
  gps_district: "UnknownLocation",
  original_name: "DSC_1231",
  extension: "JPG",
  seq: "1",
  "seq:4": "0001",
};

function localTemplatePreview(request: TemplatePreviewRequest): TemplatePreview {
  if (/\{seq(?::\d+)?\}/.test(request.directoryTemplate)) {
    throw new Error("Sequence can only be used in filename templates");
  }

  const directoryComponents = renderLocalTemplate(request.directoryTemplate)
    .split("/")
    .filter(Boolean);
  const filename = renderLocalTemplate(request.filenameTemplate);
  return {
    directoryComponents,
    filename,
    example: [...directoryComponents, filename].join("/"),
  };
}

function renderLocalTemplate(template: string): string {
  return template.replace(/\{([^}]+)\}/g, (_, key: string) => {
    const value = sampleValues[key];
    if (value === undefined) {
      throw new Error(`Unknown template variable: ${key}`);
    }
    return value;
  });
}

/**
 * 开始整理：后端重新走 scan → metadata → plan → execute 流水线，
 * 与预览共用同一 Planner 逻辑。完成后 resolve 汇总。
 */
export async function organizePhotos(): Promise<OrganizeSummary> {
  return invoke<OrganizeSummary>("organize_photos");
}

/** 请求取消任务；任务已结束时返回 false。 */
export async function cancelJob(jobId: string): Promise<boolean> {
  return invoke<boolean>("cancel_job", { jobId });
}

/** 订阅整理进度事件，返回取消订阅函数。 */
export function onOrganizeProgress(
  handler: (event: ProgressEvent) => void,
): Promise<() => void> {
  return listen<ProgressEvent>("organize-progress", (e) => handler(e.payload));
}

/** 查询上次异常退出留下的未完成任务。 */
export async function pendingRecoveryJobs(): Promise<PendingJobSummary[]> {
  return invoke<PendingJobSummary[]>("pending_recovery_jobs");
}

/** 放弃未完成任务（只改标记，绝不动文件）。 */
export async function abandonJob(jobId: string): Promise<void> {
  return invoke<void>("abandon_job", { jobId });
}

/** 保存 Google API Key 到 OS 凭据存储。 */
export async function setGoogleApiKey(key: string): Promise<void> {
  return invoke<void>("set_google_api_key", { key });
}

/** 清除已存的 Google API Key。 */
export async function clearGoogleApiKey(): Promise<void> {
  return invoke<void>("clear_google_api_key");
}

/** 查询是否已配置 API Key（不返回 Key 本身）。 */
export async function hasGoogleApiKey(): Promise<boolean> {
  return invoke<boolean>("has_google_api_key");
}

/** 连通性自检：用给定坐标反查，返回格式化地址。 */
export async function testGeocode(latitude: number, longitude: number): Promise<string> {
  return invoke<string>("test_geocode", { latitude, longitude });
}
