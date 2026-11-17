
export interface ProviderConfig {
  accounts?: ProviderAccountsConfig | null;
  resume_sessions?: boolean | null;
  default_model?: string | null;
  usage_tracking_enabled?: boolean | null;
  [key: string]: unknown;
}

export interface ProviderAccountsConfig {
  default_name?: string;
  profiles?: Record<string, { name?: string }>;
  order: string[];
  automatic_switch: boolean;
  workspaces?: Record<string, string>;
}

export interface UiConfig {
  memory_tab_enabled: boolean;
  mesh_tab_enabled: boolean;
  [key: string]: unknown;
}
