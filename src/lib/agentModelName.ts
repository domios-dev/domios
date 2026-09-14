/**
 * The catalogue-backed short name a recognised model renders as, mirrored from
 * `codeterm_types::pty::agent_model_name` so the sidebar, the daemon and the
 * CLI cannot disagree about the same tab.
 *
 * The provider catalogues (`plugins/provider-*\/plugin.json`) own the model ids;
 * this module owns the one policy that turns such an id into the short family a
 * bracket shows. The policy is structural so it survives a version bump —
 * `gpt-5.6-sol` and a later `gpt-5.9-sol` both render `sol` — and it is closed
 * over `MODEL_FAMILIES` so an id no catalogue carries is reported unrecognised
 * instead of guessed at.
 */

/** Codenames the shipped catalogues carry, lowercased. Version-free by design. */
export const MODEL_FAMILIES = [
  "opus",
  "sonnet",
  "haiku",
  "fable",
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
  const body = model.trim().toLowerCase().split("/").pop() ?? "";
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
  const family = tokens.find((token) => FAMILIES.has(token));
  if (family) return family;
  const [vendor, ...version] = tokens;
  if (!vendor || !VERSIONED_VENDORS.has(vendor) || version.length === 0) return null;
  return version.every(isVersion) ? `${vendor} ${version.join(".")}` : null;
}
