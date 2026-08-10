export interface TemplatePreviewRequest {
  directoryTemplate: string;
  filenameTemplate: string;
}

export interface TemplatePreview {
  directoryComponents: string[];
  filename: string;
  example: string;
}
