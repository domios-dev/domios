import type {OptionalWireFields} from "./wireViews";

export type ParsedSessionRecord = OptionalWireFields<import("./gen/ParsedRecord").ParsedRecord, "oversized" | "bytes">;

export type JsonlRecordFilter = OptionalWireFields<import("./gen/RecordFilter").RecordFilter, "type" | "where">;

export type JsonlRecordSource = OptionalWireFields<import("./gen/RecordSource").RecordSource, "maxRecordBytes" | "oversized">;

/** Manifest capabilities.recordSources selects the matching parsed-record hook. */
export type JsonlRecordSources = Record<string, JsonlRecordSource>;

export type SessionRecordsMeta = OptionalWireFields<import("./gen/RecordMeta").RecordMeta, "sourceKey">;
