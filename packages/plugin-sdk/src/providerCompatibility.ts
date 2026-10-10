export type ProviderCompatibilityReason =
  | "sqlite_schema_conversion"
  | "event_route_changed"
  | "server_password_required"
  | "version_unknown"
  | "version_not_declared"
  | "invalid_manifest"
  | "undeclared_capability";

export interface ProviderCompatibilityManifest {
  supported: string[];
  breaking: { range: string; reasons: ProviderCompatibilityReason[] }[];
  modelMinimumCli?: Record<string, string>;
  directSqlite?: boolean;
}
