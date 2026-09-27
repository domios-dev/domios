export interface ProjectTrustCarryHost {
  fs: {
    fileExists(path: string): boolean;
    isSymlink(path: string): boolean | null;
    readFile(path: string): string | null;
    writeFileAtomic(path: string, contents: string): boolean;
    lockFile(path: string): string | null;
    unlockFile(id: string): boolean;
  };
  log?(level: "warn", message: string): void;
}

interface TrustEntry {
  path: string;
  header: string;
  assignment: string;
}

function normalizedPath(value: string): string {
  const isWindowsPath = /^[A-Za-z]:[\\/]/.test(value)
    || value.startsWith("\\\\")
    || value.startsWith("//");
  return isWindowsPath ? value.replace(/\//g, "\\").toLowerCase() : value;
}

function isCodeTermProvisionedWorkspacePath(value: string): boolean {
  const parts = normalizedPath(value).split(/[\\/]+/).filter(Boolean);
  return parts.some((part, index) =>
    /^\.codeterm(?:-dev(?:-[a-z0-9-]+)?)?$/.test(part)
    && parts[index + 1] === "provisioned-workspaces",
  );
}

function parseString(value: string): { value: string; next: number } | null {
  const quote = value[0];
  if (quote !== "'" && quote !== '"') return null;
  for (let i = 1; i < value.length; i++) {
    if (quote === '"' && value[i] === "\\") {
      i++;
      continue;
    }
    if (value[i] !== quote) continue;
    const raw = value.slice(0, i + 1);
    try {
      return { value: quote === '"' ? JSON.parse(raw) : raw.slice(1, -1), next: i + 1 };
    } catch {
      return null;
    }
  }
  return null;
}

function parseDottedKey(value: string): string[] | null {
  const parts: string[] = [];
  let index = 0;
  while (index < value.length) {
    while (/\s/.test(value[index] || "")) index++;
    if (index >= value.length) break;
    let part: string;
    if (value[index] === "'" || value[index] === '"') {
      const parsed = parseString(value.slice(index));
      if (!parsed) return null;
      part = parsed.value;
      index += parsed.next;
    } else {
      const match = /^[A-Za-z0-9_-]+/.exec(value.slice(index));
      if (!match) return null;
      part = match[0];
      index += part.length;
    }
    parts.push(part);
    while (/\s/.test(value[index] || "")) index++;
    if (index === value.length) return parts;
    if (value[index] !== ".") return null;
    index++;
  }
  return parts.length ? parts : null;
}

function withoutComment(value: string): string {
  let quote = "";
  for (let i = 0; i < value.length; i++) {
    const char = value[i];
    if (quote === '"' && char === "\\") {
      i++;
      continue;
    }
    if (quote) {
      if (char === quote) quote = "";
      continue;
    }
    if (char === "'" || char === '"') quote = char;
    else if (char === "#") return value.slice(0, i);
  }
  return value;
}

function headerParts(line: string): string[] | null {
  const value = withoutComment(line).trim();
  if (!value.startsWith("[") || !value.endsWith("]") || value.startsWith("[[")) return null;
  return parseDottedKey(value.slice(1, -1));
}

function assignmentParts(line: string): { key: string[]; value: string } | null {
  const value = withoutComment(line);
  let quote = "";
  for (let i = 0; i < value.length; i++) {
    const char = value[i];
    if (quote === '"' && char === "\\") {
      i++;
      continue;
    }
    if (quote) {
      if (char === quote) quote = "";
      continue;
    }
    if (char === "'" || char === '"') quote = char;
    else if (char === "=") {
      const key = parseDottedKey(value.slice(0, i));
      return key ? { key, value: value.slice(i + 1).trim() } : null;
    }
  }
  return null;
}

function parseTrustValue(value: string): "trusted" | "untrusted" | null {
  const parsed = parseString(value.trim());
  if (!parsed || parsed.next !== value.trim().length) return null;
  return parsed.value === "trusted" || parsed.value === "untrusted" ? parsed.value : null;
}

function projectTablePath(parts: string[] | null): string | null {
  return parts?.length === 2 && parts[0] === "projects" ? parts[1] : null;
}

function collectProfileEntries(profile: string): TrustEntry[] | null {
  const lines = profile.split(/\r?\n/);
  const entries = new Map<string, TrustEntry>();
  const seenProjectPaths = new Set<string>();
  let activePath: string | null = null;
  let activeHeader = "";
  let trust: { line: string } | null = null;
  let trustSeen = false;
  const flush = (): boolean => {
    if (activePath === null || !trust) return true;
    const key = normalizedPath(activePath);
    if (entries.has(key)) return false;
    if (!isCodeTermProvisionedWorkspacePath(activePath)) {
      entries.set(key, { path: activePath, header: activeHeader, assignment: trust.line });
    }
    return true;
  };
  for (const line of lines) {
    const parts = headerParts(line);
    if (parts) {
      if (!flush()) return null;
      activePath = projectTablePath(parts);
      activeHeader = activePath === null ? "" : withoutComment(line).trim();
      if (activePath !== null) {
        const key = normalizedPath(activePath);
        if (seenProjectPaths.has(key)) return null;
        seenProjectPaths.add(key);
      }
      trust = null;
      trustSeen = false;
      continue;
    }
    const malformedHeader = withoutComment(line).trim();
    if (malformedHeader.startsWith("[")) {
      if (/^\[\[\s*projects(?:\s*\.|\s*\])/.test(malformedHeader)
        || /^\[\s*projects(?:\s*\.|\s*\])/.test(malformedHeader)) return null;
      if (!flush()) return null;
      activePath = null;
      activeHeader = "";
      trust = null;
      trustSeen = false;
      continue;
    }
    if (activePath === null) continue;
    const assignment = assignmentParts(line);
    if (!assignment || assignment.key.length !== 1 || assignment.key[0] !== "trust_level") continue;
    if (trustSeen) return null;
    trustSeen = true;
    const parsed = parseTrustValue(assignment.value);
    if (parsed) trust = { line: line.replace(/\r$/, "") };
  }
  return flush() ? [...entries.values()] : null;
}

function inlineTableKeys(value: string): string[] | null {
  const trimmed = value.trim();
  if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return null;
  const body = trimmed.slice(1, -1);
  const keys: string[] = [];
  let start = 0;
  let quote = "";
  let depth = 0;
  const add = (segment: string): boolean => {
    if (!segment.trim()) return true;
    const assignment = assignmentParts(segment);
    if (!assignment || !assignment.key.length) return false;
    keys.push(assignment.key[0]);
    return true;
  };
  for (let i = 0; i < body.length; i++) {
    const char = body[i];
    if (quote === '"' && char === "\\") {
      i++;
      continue;
    }
    if (quote) {
      if (char === quote) quote = "";
      continue;
    }
    if (char === "'" || char === '"') quote = char;
    else if (char === "{" || char === "[") depth++;
    else if (char === "}" || char === "]") depth--;
    else if (char === "," && depth === 0) {
      if (!add(body.slice(start, i))) return null;
      start = i + 1;
    }
  }
  if (!add(body.slice(start))) return null;
  return keys;
}

function collectExistingPaths(config: string): { paths: Set<string>; rootInline: Set<string> | null; invalid: boolean } {
  const lines = config.split(/\r?\n/);
  const paths = new Set<string>();
  const directTables = new Set<string>();
  const nestedTables = new Set<string>();
  const projectHeaders = new Set<string>();
  const parentKeys = new Set<string>();
  let currentTable: string[] = [];
  let rootInline: Set<string> | null = null;
  let projectsTableSeen = false;
  let invalid = false;
  for (const line of lines) {
    const parts = headerParts(line);
    if (parts) {
      currentTable = parts;
      if (parts[0] === "projects") {
        if (rootInline) invalid = true;
        if (parts.length === 1) {
          if (projectsTableSeen) invalid = true;
          projectsTableSeen = true;
        } else {
          const key = normalizedPath(parts[1]);
          const headerKey = JSON.stringify([key, ...parts.slice(2)]);
          if (projectHeaders.has(headerKey)) invalid = true;
          projectHeaders.add(headerKey);
          paths.add(key);
          if (parts.length === 2) {
            if (directTables.has(key) || parentKeys.has(key)) invalid = true;
            directTables.add(key);
          } else {
            nestedTables.add(key);
          }
        }
      }
      continue;
    }
    const relevantHeader = withoutComment(line).trim();
    if (relevantHeader.startsWith("[")) {
      if (/^\[\[\s*projects(?:\s*\.|\s*\])/.test(relevantHeader)
        || /^\[\s*projects(?:\s*\.|\s*\])/.test(relevantHeader)) invalid = true;
      currentTable = [];
      continue;
    }
    const assignment = assignmentParts(line);
    if (!assignment) {
      if ((currentTable.length === 1 && currentTable[0] === "projects")
        && relevantHeader.length > 0 && !relevantHeader.startsWith("#")) invalid = true;
      if (currentTable.length === 0 && /^projects\s*=/.test(relevantHeader)) invalid = true;
      continue;
    }
    if (currentTable.length === 0 && assignment.key[0] === "projects") {
      if (assignment.key.length === 1) {
        const keys = inlineTableKeys(assignment.value);
        if (!keys || rootInline) {
          invalid = true;
          continue;
        }
        rootInline = new Set();
        for (const key of keys) {
          const normalized = normalizedPath(key);
          if (rootInline.has(normalized)) invalid = true;
          rootInline.add(normalized);
          paths.add(normalized);
        }
      } else {
        const key = normalizedPath(assignment.key[1]);
        if (parentKeys.has(key) || directTables.has(key)) invalid = true;
        parentKeys.add(key);
        paths.add(key);
      }
      continue;
    }
    if (currentTable.length === 1 && currentTable[0] === "projects") {
      const key = normalizedPath(assignment.key[0]);
      if (parentKeys.has(key) || directTables.has(key)) invalid = true;
      parentKeys.add(key);
      paths.add(key);
    }
  }
  for (const key of parentKeys) {
    if (nestedTables.has(key)) invalid = true;
  }
  if (rootInline && (projectsTableSeen || directTables.size > 0 || nestedTables.size > 0 || parentKeys.size > 0)) invalid = true;
  return { paths, rootInline, invalid };
}

export function mergeTrustedProjectEntries(profile: string, config: string): string | null {
  const entries = collectProfileEntries(profile);
  if (!entries) return null;
  if (entries.length === 0) return config;
  const existing = collectExistingPaths(config);
  if (existing.invalid) return null;
  const missing = entries.filter((entry) => {
    const key = normalizedPath(entry.path);
    return !existing.paths.has(key) && !existing.rootInline?.has(key);
  });
  if (existing.rootInline && missing.length) return null;
  if (missing.length === 0) return config;
  const newlineSource = config || profile;
  const newline = newlineSource.includes("\r\n") ? "\r\n" : "\n";
  const separator = !config ? "" : config.endsWith(newline + newline)
    ? ""
    : config.endsWith(newline) ? newline : newline + newline;
  const appended = missing.flatMap((entry) => [entry.header, entry.assignment]).join(newline) + newline;
  return config + separator + appended;
}

export function carryCodexProjectTrust(
  host: ProjectTrustCarryHost,
  profilePath: string,
  configPath: string,
): boolean {
  const lockId = host.fs.lockFile(configPath + ".project-trust.lock");
  if (lockId === null) {
    host.log?.("warn", "Codex project trust carry-over skipped because the shared config lock is busy");
    return false;
  }
  try {
    if (host.fs.isSymlink(configPath) !== false) {
      host.log?.("warn", "Codex project trust carry-over skipped because the shared config path could not be safely replaced");
      return false;
    }
    const profile = host.fs.readFile(profilePath);
    if (profile === null) return false;
    const configExists = host.fs.fileExists(configPath);
    const config = configExists ? host.fs.readFile(configPath) : "";
    if (config === null) return false;
    const merged = mergeTrustedProjectEntries(profile, config);
    if (merged === null) {
      host.log?.("warn", "Codex project trust carry-over skipped because the TOML structure is ambiguous");
      return false;
    }
    if (merged === config) return true;
    const currentExists = host.fs.fileExists(configPath);
    const current = currentExists ? host.fs.readFile(configPath) : "";
    if (current === null || currentExists !== configExists || current !== config) {
      host.log?.("warn", "Codex project trust carry-over skipped after a concurrent config change");
      return false;
    }
    if (host.fs.isSymlink(configPath) !== false) {
      host.log?.("warn", "Codex project trust carry-over skipped because the shared config path could not be safely replaced");
      return false;
    }
    if (!host.fs.writeFileAtomic(configPath, merged)) {
      host.log?.("warn", "Codex project trust carry-over could not atomically update the shared config");
      return false;
    }
    return true;
  } finally {
    host.fs.unlockFile(lockId);
  }
}
