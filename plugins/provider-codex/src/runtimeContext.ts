import type { JsonValue, RuntimeRestoreContext } from "@codeterm/plugin-sdk";
import { object } from "./runtimeProtocol";

function invalid(message: string): never {
  throw {code:"invalid_schema",message};
}
function exactObject(raw: unknown, keys: readonly string[]): Record<string, unknown> {
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) invalid("Invalid restoreContext object");
  const value = object(raw);
  if (Object.keys(value).length !== keys.length || keys.some(key=>!Object.prototype.hasOwnProperty.call(value,key))) invalid("Invalid restoreContext fields");
  return value;
}
export function restoreContextItems(raw: unknown): JsonValue[] {
  const value = exactObject(raw,["instructions","additionalContext"]);
  if (value.instructions !== null && typeof value.instructions !== "string") invalid("Invalid restoreContext instructions");
  if (!Array.isArray(value.additionalContext)) invalid("Invalid restoreContext additionalContext");
  const entries = new Map<string, RuntimeRestoreContext["additionalContext"][number]>();
  for (const rawEntry of value.additionalContext) {
    const entry = exactObject(rawEntry,["key","kind","value"]);
    if (typeof entry.key !== "string" || typeof entry.value !== "string" || (entry.kind !== "application" && entry.kind !== "user")) invalid("Invalid restoreContext entry");
    const previous = entries.get(entry.key);
    if (previous && (previous.kind !== entry.kind || previous.value !== entry.value)) invalid("Conflicting restoreContext key");
    entries.set(entry.key,entry as RuntimeRestoreContext["additionalContext"][number]);
  }
  const context = value as unknown as RuntimeRestoreContext;
  const item = (role:string,text:string): JsonValue => ({type:"message",role,content:[{type:"input_text",text}]});
  const items:JsonValue[] = context.instructions ? [item("developer",context.instructions)] : [];
  for (const entry of entries.values()) items.push(item(entry.kind === "application" ? "developer" : "user",entry.value));
  if (!items.length) invalid("Empty restoreContext");
  return items;
}
