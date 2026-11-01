export interface ExecutionSandboxMode {
  launchArgs: string[];
  runtimeMode: string | null;
}

export interface ExecutionSandboxCapability {
  platforms?: string[];
  readOnly?: ExecutionSandboxMode | null;
  runtimeReadOnlyWritableRootsMode?: string | null;
  mutating?: ExecutionSandboxMode | null;
  unsupportedReason?: string | null;
  forbiddenArgumentPrefixes?: string[];
}
