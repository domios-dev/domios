import { checkProviderUpdate, type CliCompatibility } from "../../provider-shared/updateCheck";

export function checkOpencodeUpdate() {
  let manifest: { cliCompatibility?: CliCompatibility };
  try { manifest = JSON.parse(host.manifestJson() ?? "null"); } catch { return null; }
  if (!manifest?.cliCompatibility?.supported?.length) return null;
  return checkProviderUpdate(host, "opencode", "https://registry.npmjs.org/opencode-ai", manifest.cliCompatibility);
}
