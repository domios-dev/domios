export interface ProviderNotInstalledInfo {
  providerId: string;
  displayName: string;
  binary: string;
  installCommand: string | null;
}

const NO_INSTALL_COMMAND = "No install command is available for this platform.";

const PROVIDER_NOT_INSTALLED =
  /Provider not installed:\s+(.+?)\s+\(`([^`]+)`\)\s+launches\s+`([^`]+)`[^]*?Install command:\s*(.+?)\.\s+fix:\s+domios agent cli install\s+\S+/;

export function parseProviderNotInstalled(input: unknown): ProviderNotInstalledInfo | null {
  const raw = typeof input === "string"
    ? input
    : input instanceof Error
      ? input.message
      : typeof input === "object" && input !== null && "message" in input && typeof (input as { message?: unknown }).message === "string"
        ? (input as { message: string }).message
        : "";
  if (!raw) return null;
  const match = raw.match(PROVIDER_NOT_INSTALLED);
  if (!match) return null;
  const displayName = match[1].trim();
  const providerId = match[2].trim();
  const binary = match[3].trim();
  const rawCommand = match[4].trim();
  if (!displayName || !providerId || !binary) return null;
  const installCommand = rawCommand && rawCommand !== NO_INSTALL_COMMAND ? rawCommand : null;
  return { providerId, displayName, binary, installCommand };
}
