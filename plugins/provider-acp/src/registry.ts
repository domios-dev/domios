import { ioResolve, type HostIoSteps } from "@codeterm/plugin-sdk";
export const registryBounds = {bytes: 2 * 1024 * 1024, agents: 512, string: 4096, args: 128} as const;
export interface PackageDistribution {package: string; args: string[]; env: Record<string, string>}
export interface BinaryDistribution {archive: string; cmd: string; args: string[]; env: Record<string, string>; sha256?: string}
export interface RegistryAgent {
  id: string; name: string; version: string; description: string; icon?: string;
  distribution: {npx?: PackageDistribution; uvx?: PackageDistribution; binary?: Record<string, BinaryDistribution>};
}
export interface RegistryIndex {version: "1.0.0"; agents: RegistryAgent[]}
const targets = new Set(["darwin-aarch64", "darwin-x86_64", "linux-aarch64", "linux-x86_64", "windows-aarch64", "windows-x86_64"]);
function invalid(): never {throw new Error("Invalid or oversized ACP registry v1 index");}
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalid();
  return value as Record<string, unknown>;
}
function fields(value: Record<string, unknown>, allowed: readonly string[]): void {
  if (Object.keys(value).some(key => !allowed.includes(key))) invalid();
}
function string(value: unknown, limit: number = registryBounds.string): string {
  if (typeof value !== "string" || !value.length || value.length > limit || /[\u0000-\u001f]/.test(value)) invalid();
  return value;
}
function args(value: unknown): string[] {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > registryBounds.args) invalid();
  return value.map(value => value === "" ? "" : string(value));
}
function env(value: unknown): Record<string, string> {
  if (value === undefined) return {};
  const entries = Object.entries(object(value));
  if (entries.length > 128) invalid();
  for (const [key, value] of entries) {
    if (!/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(key) || key.toUpperCase().startsWith("CODETERM")) invalid();
    if (value !== "") string(value);
  }
  return Object.fromEntries(entries) as Record<string, string>;
}
function url(value: unknown): string {
  const text = string(value);
  if (!/^https:\/\/[^/@\s?#]+(?:\/[^\s#]*)?$/.test(text)) invalid();
  return text;
}
function distribution(value: unknown): RegistryAgent["distribution"] {
  const raw = object(value);
  fields(raw, ["binary", "npx", "uvx"]);
  if (!Object.keys(raw).length) invalid();
  const result: RegistryAgent["distribution"] = {};
  for (const kind of ["npx", "uvx"] as const) {
    if (raw[kind] === undefined) continue;
    const entry = object(raw[kind]);
    fields(entry, ["package", "args", "env"]);
    result[kind] = {package: string(entry.package, 256), args: args(entry.args), env: env(entry.env)};
  }
  if (raw.binary !== undefined) {
    const entries = Object.entries(object(raw.binary));
    if (!entries.length || entries.length > targets.size) invalid();
    result.binary = Object.fromEntries(entries.map(([target, value]) => {
      if (!targets.has(target)) invalid();
      const entry = object(value);
      fields(entry, ["archive", "cmd", "args", "env", "sha256"]);
      const sha256 = entry.sha256 === undefined ? undefined : string(entry.sha256, 64);
      if (sha256 !== undefined && !/^[a-f\d]{64}$/i.test(sha256)) invalid();
      return [target, {archive: url(entry.archive), cmd: string(entry.cmd), args: args(entry.args), env: env(entry.env), ...(sha256 ? {sha256} : {})}];
    }));
  }
  return result;
}
function utf8Bytes(value: string): number {
  let size = 0;
  for (const character of value) {
    const point = character.codePointAt(0)!;
    size += point < 128 ? 1 : point < 2048 ? 2 : point < 65536 ? 3 : 4;
    if (size > registryBounds.bytes) invalid();
  }
  return size;
}
export function decodeRegistry(text: string): RegistryIndex {
  if (text.length > registryBounds.bytes || utf8Bytes(text) > registryBounds.bytes) invalid();
  const index = object(JSON.parse(text));
  fields(index, ["version", "agents"]);
  if (index.version !== "1.0.0" || !Array.isArray(index.agents) || index.agents.length > registryBounds.agents) invalid();
  const ids = new Set<string>();
  const agents = index.agents.map(value => {
    const entry = object(value);
    fields(entry, ["id", "name", "version", "description", "distribution", "repository", "website", "authors", "license", "license_url", "icon"]);
    const id = string(entry.id, 128);
    if (!/^[a-z][a-z0-9-]*$/.test(id) || ids.has(id)) invalid();
    ids.add(id);
    const version = string(entry.version, 128);
    if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(version)) invalid();
    for (const field of ["repository", "website", "license_url", "icon"]) if (entry[field] !== undefined) url(entry[field]);
    if (entry.license !== undefined) string(entry.license);
    if (entry.authors !== undefined && (!Array.isArray(entry.authors) || entry.authors.length > 64)) invalid();
    if (Array.isArray(entry.authors)) entry.authors.forEach(author => string(author));
    return {id, ...(entry.icon === undefined ? {} : {icon: url(entry.icon)}), name: string(entry.name, 256), version, description: string(entry.description), distribution: distribution(entry.distribution)};
  });
  return {version: "1.0.0", agents};
}
export interface RegistryCachePort {read(): string | null | HostIoSteps<string | null>; write(text: string): boolean | HostIoSteps<boolean>}
export function* searchRegistryCache(cache: RegistryCachePort, query: string): HostIoSteps<RegistryAgent[]> {
  const text = yield* ioResolve(cache.read());
  if (text === null) return [];
  const needle = query.trim().slice(0, 256).toLowerCase();
  return decodeRegistry(text).agents.filter(agent => `${agent.id} ${agent.name} ${agent.description}`.toLowerCase().includes(needle));
}
export function* saveRegistryCache(cache: RegistryCachePort, text: string): HostIoSteps<void> {
  decodeRegistry(text);
  if (!(yield* ioResolve(cache.write(text)))) throw new Error("ACP registry cache could not be saved");
}
