/**
 * The catalogue-backed short name a recognised model renders as, mirrored from
 * `codeterm_types::pty::agent_model_name` so the sidebar, the daemon and the
 * CLI cannot disagree about the same tab.
 *
 * The provider catalogues (`plugins/provider-*\/plugin.json`) own the model ids;
 * this module owns the one policy that turns such an id into the short family a
 * bracket shows. The policy is structural so it survives a version bump —
 * `gpt-5.6-sol` and `gpt-5.9-sol` keep their dotted versions — and it is closed
 * over `MODEL_FAMILIES` so an id no catalogue carries is reported unrecognised
 * instead of guessed at.
 */

/** Catalogue aliases and family tokens, lowercased. */
export const MODEL_FAMILIES = [
  "opus",
  "sonnet",
  "haiku",
  "fable",
  "best",
  "opusplan",
  "astra",
  "sol",
  "terra",
  "luna",
  "spark",
  "mini",
] as const;

/**
 * Vendors whose catalogue carries un-codenamed entries (`gpt-5.5`), rendered as
 * `<vendor> <version>` because the version IS the identity there.
 */
const VERSIONED_VENDORS = new Set(["gpt"]);

/** Tokens that name the product line rather than the model. */
const NOISE_TOKENS = new Set(["codex"]);

const FAMILIES = new Set<string>(MODEL_FAMILIES);

function tokenize(model: string): string[] {
  const leaf = model.trim().toLowerCase().split("/").pop() ?? "";
  const body = leaf.replace(/\[1m\]$/, "");
  return body
    .split(/[-_\s]+/)
    .filter((token) => token.length > 0 && !NOISE_TOKENS.has(token));
}

function isVersion(token: string): boolean {
  return /^[0-9.]+$/.test(token) && /[0-9]/.test(token);
}

/**
 * The short name for a catalogue model, or `null` when no catalogue claims it.
 * Pure: it only decides what to render and never speaks for the stored value.
 */
export function shortModelName(model: string): string | null {
  const tokens = tokenize(model);
  const familyIndex = tokens.findIndex((token) => FAMILIES.has(token));
  if (familyIndex >= 0) {
    const family = tokens[familyIndex];
    if (tokens[0] === "gpt") {
      const version = tokens.slice(0, familyIndex).reverse().find(isVersion);
      if (version?.includes(".")) return `${version}-${family}`;
    }
    return family;
  }
  const [vendor, ...version] = tokens;
  if (!vendor || !VERSIONED_VENDORS.has(vendor) || version.length === 0) return null;
  return version.every(isVersion) ? `${vendor} ${version.join(".")}` : null;
}
