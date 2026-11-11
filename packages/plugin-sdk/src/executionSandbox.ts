import type {OptionalWireFields} from "./wireViews";
export type ExecutionSandboxMode = import("./gen/ExecutionSandboxMode").ExecutionSandboxMode;

export type ExecutionSandboxCapability = OptionalWireFields<import("./gen/ExecutionSandboxCapability").ExecutionSandboxCapability, "platforms" | "readOnly" | "runtimeReadOnlyWritableRootsMode" | "mutating" | "unsupportedReason" | "forbiddenArgumentPrefixes">;
