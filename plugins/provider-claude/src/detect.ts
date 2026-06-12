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

// Claude working spinner uses Braille chars U+2800..U+28FF in status lines.
// Match the full range to keep pace with future glyph additions.
export const titlePattern = /claude|^[⠀-⣿✱✳✻] /i;

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

const DETECTION_THRESHOLD = 2;
const LITERAL_SIGNALS = [
  "? for shortcuts",
  "esc to interrupt",
  "Claude Code",
  "Do you want to proceed?",
  "may I proceed",
  "Would you like me to",
  "Yes, and don't ask again",
  "Enter to confirm",
  "Cogitated for",
  "Brewed for",
  "Reasoned for",
];

function hasSpinnerGlyph(text: string): boolean {
  return /(?:^|\n)\s*[⠀-⣿✱✳✻]\s+\S/.test(text);
}

function hasFooterLayout(text: string): boolean {
  return /(?:^|\n).*(?:\? for shortcuts|for shortcuts).*(?:esc to interrupt|shift\+tab|ctrl\+g)/i.test(text);
}

function hasPromptOptionLayout(text: string): boolean {
  const numbered = text.match(/^\s*(?:❯\s*)?\d+\.\s+\S+/gm) ?? [];
  return numbered.length >= 2 && (text.includes("❯") || /Esc .*?(cancel|back)|Enter .*?confirm|Tab .*?amend/is.test(text));
}

function outputScore(text: string): number {
  let score = 0;
  if (hasSpinnerGlyph(text)) score += 2;
  if (hasFooterLayout(text)) score += 1;
  if (hasPromptOptionLayout(text)) score += 2;
  for (const literal of LITERAL_SIGNALS) {
    if (text.includes(literal)) score += 1;
  }
  return score;
}

export function detect(sample: DetectionSample): boolean {
  if (sample.title && titlePattern.test(sample.title)) return true;
  return outputScore(sample.bytes) >= DETECTION_THRESHOLD;
}
