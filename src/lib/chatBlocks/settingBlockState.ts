import { isSensitiveKey, type SettingBlockState, type SettingControl, type SettingValue } from "@codeterm/chat-ui";
import type { SettingField } from "../types";

export type SettingNamespace = "plugins" | "agents";

/** What the owner's settings schema source knows about one plugin or agent provider. */
export type SchemaLookup =
  | { status: "loading" }
  | { status: "unavailable"; message: string }
  | { status: "missing" }
  | { status: "ready"; owner: string; schema: readonly SettingField[] };

type ValueField = Extract<SettingField, { kind: "toggle" | "select" | "number" | "text" }>;

const NAMESPACES: readonly SettingNamespace[] = ["plugins", "agents"];

/** `plugins.<id>.<field>` or `agents.<id>.<field>`; every other key has no schema to render from. */
export function splitSettingKey(key: string): { namespace: SettingNamespace; ownerId: string; fieldKey: string } | null {
  const parts = key.split(".");
  if (parts.length !== 3) return null;
  const [namespace, ownerId, fieldKey] = parts;
  return NAMESPACES.includes(namespace as SettingNamespace) ? { namespace: namespace as SettingNamespace, ownerId, fieldKey } : null;
}

export function findSchemaField(schema: readonly SettingField[], fieldKey: string): SettingField | null {
  for (const field of schema) {
    if (field.kind === "section") {
      const nested = findSchemaField(field.fields, fieldKey);
      if (nested) return nested;
    } else if (("key" in field && field.key === fieldKey) || (field.kind === "api_key" && field.env_var === fieldKey)) {
      return field;
    }
  }
  return null;
}

function isValueField(field: SettingField): field is ValueField {
  return field.kind === "toggle" || field.kind === "select" || field.kind === "number" || field.kind === "text";
}

function isWhole(value: number | undefined): boolean {
  return value === undefined || Number.isInteger(value);
}

export function settingControl(field: ValueField): SettingControl {
  switch (field.kind) {
    case "toggle":
      return { kind: "switch" };
    case "select":
      return { kind: "choice", options: field.options.map(([value, label]) => ({ value, label })) };
    case "number":
      return { kind: "stepper", min: field.min, max: field.max, step: isWhole(field.default) && isWhole(field.min) && isWhole(field.max) ? 1 : 0.1 };
    case "text":
      return { kind: "text", placeholder: field.placeholder };
  }
}

function fallbackValue(field: ValueField): SettingValue {
  switch (field.kind) {
    case "toggle":
      return field.default;
    case "select":
      return field.default ?? field.options[0]?.[0] ?? "";
    case "number":
      return field.default ?? field.min ?? 0;
    case "text":
      return field.default ?? "";
  }
}

function coerce(field: ValueField, raw: unknown): SettingValue {
  switch (field.kind) {
    case "toggle":
      return typeof raw === "boolean" ? raw : fallbackValue(field);
    case "number":
      return typeof raw === "number" && Number.isFinite(raw) ? raw : fallbackValue(field);
    default:
      return typeof raw === "string" ? raw : fallbackValue(field);
  }
}

function storedValue(config: unknown, namespace: SettingNamespace, ownerId: string, fieldKey: string): unknown {
  const section = (config as Record<string, unknown> | null)?.[namespace] as Record<string, Record<string, unknown> | undefined> | undefined;
  return section?.[ownerId]?.[fieldKey];
}

const UNKNOWN: SettingBlockState = { kind: "refused", reason: "unknown" };
const SENSITIVE: SettingBlockState = { kind: "refused", reason: "sensitive" };
const LOADING: SettingBlockState = { kind: "loading" };

/** The block's state for one key: never a guess while the schema or config is unknown, never a secret. */
export function settingBlockState(input: {
  key: string;
  config: unknown;
  configError: string | null;
  lookup(namespace: SettingNamespace, ownerId: string): SchemaLookup;
}): SettingBlockState {
  if (isSensitiveKey(input.key)) return SENSITIVE;
  const parts = splitSettingKey(input.key);
  if (!parts) return UNKNOWN;
  const lookup = input.lookup(parts.namespace, parts.ownerId);
  if (lookup.status === "loading") return LOADING;
  if (lookup.status === "unavailable") return { kind: "unavailable", message: lookup.message };
  if (lookup.status === "missing") return UNKNOWN;
  const field = findSchemaField(lookup.schema, parts.fieldKey);
  if (field?.kind === "api_key") return SENSITIVE;
  if (!field || !isValueField(field)) return UNKNOWN;
  if (!input.config) return input.configError ? { kind: "unavailable", message: input.configError } : LOADING;
  return {
    kind: "ready",
    setting: {
      key: input.key,
      label: field.label,
      description: field.description,
      owner: lookup.owner,
      control: settingControl(field),
      value: coerce(field, storedValue(input.config, parts.namespace, parts.ownerId, parts.fieldKey)),
    },
  };
}
