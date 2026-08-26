/**
 * Compile-time build stamps injected by Vite `define` (see vite.config.ts).
 * `define` substitutes the bare identifier only — reading the same name as a
 * property of globalThis is never substituted and silently yields the unbuilt
 * fallback. The typeof guard keeps the genuinely unbuilt case (tsx test runs,
 * plain node) from throwing a ReferenceError.
 */

export function buildHash(): string {
  return typeof __BUILD_HASH__ === "string" ? __BUILD_HASH__ : "dev";
}

export function appVersion(): string {
  return typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "dev";
}
