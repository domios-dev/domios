import type {OptionalWireFields} from "./wireViews";
export type ProviderCompatibilityReason = import("./gen/CompatibilityReason").CompatibilityReason;

export type ProviderCompatibilityManifest = OptionalWireFields<import("./gen/CompatibilityManifest").CompatibilityManifest, "modelMinimumCli" | "directSqlite">;
