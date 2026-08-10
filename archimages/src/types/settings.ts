/** 界面语言。 */
export type AppLocale = "zh-CN" | "en";

/** 与 Rust models/settings.rs 的 serde(camelCase) 契约一一对应。 */
export type OperationMode = "copy" | "move" | "copyVerifyDelete";
export type DuplicateMode = "modern" | "legacyStrict";
export type GpsPathLevel =
  | "country"
  | "province"
  | "city"
  | "district"
  | "formattedAddress";
export type ThemeMode = "system" | "light" | "dark";
/** 无 Google API Key 时 GPS 的降级策略（P6 起后端字段）。 */
export type GpsNoApiMode = "ignore" | "coordinates" | "unknownLocation";

export interface MetadataFallback {
  useModifiedTime: boolean;
  unknownCamera: string;
  unknownLocation: string;
  unknownDate: string;
}

export interface AppSettings {
  sourceDirectory: string | null;
  destinationDirectory: string | null;
  includeSubfolders: boolean;
  operationMode: OperationMode;
  directoryTemplate: string;
  filenameTemplate: string;
  duplicateMode: DuplicateMode;
  gpsEnabled: boolean;
  gpsPathLevel: GpsPathLevel;
  gpsRoundPrecision: number;
  gpsNoApiMode: GpsNoApiMode;
  metadataFallback: MetadataFallback;
  maxHashWorkers: number;
  maxCopyWorkers: number;
  theme: ThemeMode;
  language: string;
}
