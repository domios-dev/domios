
export interface ProviderConfig {
  resume_sessions?: boolean | null;
  default_model?: string | null;
  usage_tracking_enabled?: boolean | null;
  [key: string]: unknown;
}

export interface UiConfig {
  memory_tab_enabled: boolean;
  mesh_tab_enabled: boolean;
  [key: string]: unknown;
}
