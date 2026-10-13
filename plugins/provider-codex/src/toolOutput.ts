import { canonicalJson } from "./canonicalJson";
export function codexToolOutputText(output: unknown): string {
  if (typeof output === "string") return output;
  if (output == null) return "";
  if (Array.isArray(output) && output.every((item) => item && typeof item === "object" && typeof item.text === "string")) {
    return output.map((item: { text: string }) => item.text).join("");
  }
  return typeof output === "object" ? canonicalJson(output) : String(output);
}
