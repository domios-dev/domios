
export interface ProviderConfig {
  accounts?: ProviderAccountsConfig | null;
  resume_sessions?: boolean | null;
  default_model?: string | null;
  usage_tracking_enabled?: boolean | null;
  [key: string]: unknown;
}

export interface ProviderAccountsConfig {
  default_name?: string;
  default_identity?: import("../../gen/AccountIdentity").AccountIdentity;
  profiles?: Record<string, { name?: string; identity?: import("../../gen/AccountIdentity").AccountIdentity; credential_namespace?: string; retained_namespaces?: string[]; removal?: {id:string;failure:string} }>;
  order: string[];
  automatic_switch: boolean;
  workspaces?: Record<string, string>;
}

export interface UiConfig {
  memory_tab_enabled: boolean;
  mesh_tab_enabled: boolean;
  [key: string]: unknown;
}
