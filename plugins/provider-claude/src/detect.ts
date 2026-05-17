/**
 * Detection patterns and noise filters for Claude Code.
 *
 * `titlePattern` matches Claude's startup banner, splash screen, and
 * the Braille-dot working spinner (`⠂ Task name…`).
 *
 * `noisePatterns` are TUI artefacts dropped from the fallback PTY
 * parser so they don't pollute the chat view.
 *
 * `detect()` is called by the plugin registry to fingerprint fresh
 * panes — it returns true when the sample looks like a Claude session.
 */

import type { DetectionSample } from "../../../src/lib/plugins/types";

export const titlePattern =
  /claude|^[⠁⠂⠄⠈⠐⠠⡀⠘⠃⠅⠑⠒⠔⠕⠡⠢⠤⠥⠨⠱⠲⠴⠸✱✻] /i;

export const noisePatterns: RegExp[] = [
  /bypass permissions on/i,
  /shift\+tab to cycle/i,
  /Update available!.*brew upgrade/i,
  /esc to interrupt/i,
  /\? for shortcuts/i,
  /ctrl\+g to edit/i,
  /Monthly spend limit/i,
  /hold Space/i,
  /^[\s─━═]+$/,
  /^👍$/,
  /[✱✳⏳]\s*\w+[….]{0,3}$/,
  /Esc to cancel.*Tab to amend/s,
  /Enter to confirm.*Esc to cancel/s,
];

export function detect(sample: DetectionSample): boolean {
  if (sample.title && titlePattern.test(sample.title)) return true;
  // PTY-byte fingerprint — Claude's TUI footer is the cheapest signal
  // we can match on without false positives against plain shells.
  if (
    sample.bytes.includes("? for shortcuts") ||
    sample.bytes.includes("esc to interrupt") ||
    sample.bytes.includes("✱ ") ||
    sample.bytes.includes("✻ ")
  ) {
    return true;
  }
  return false;
}
