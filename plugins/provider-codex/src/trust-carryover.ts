export interface ProjectTrustCarryHost {
  fs: {
    fileExists(path: string): boolean;
    readFile(path: string): string | null;
    writeFileAtomic(path: string, contents: string): boolean;
    lockFile(path: string): string | null;
    unlockFile(id: string): boolean;
  };
}

function projectHeader(line: string): string | null {
  const value = line.trim();
  return /^\[projects\.(?:'[^'\r\n]+'|"[^"\r\n]+")\]$/.test(value) ? value : null;
}

function trustedAssignment(line: string): boolean {
  return /^\s*trust_level\s*=\s*(['"])trusted\1\s*(?:#.*)?\r?$/.test(line);
}

function trustAssignment(line: string): boolean {
  return /^\s*trust_level\s*=/.test(line);
}

function replaceTrustAssignment(line: string): string | null {
  const match = /^(\s*trust_level\s*=\s*)(?:"[^"\r\n]*"|'[^'\r\n]*')(\s*(?:#.*)?\r?)$/.exec(line);
  return match ? `${match[1]}"trusted"${match[2]}` : null;
}

function nextTable(lines: string[], from: number): number {
  for (let i = from; i < lines.length; i++) {
    const value = lines[i].trim();
    if (value.startsWith("[") && value.endsWith("]")) return i;
  }
  return lines.length;
}

export function mergeTrustedProjectEntries(profile: string, config: string): string | null {
  const profileLines = profile.split(/\r?\n/);
  const entries = new Map<string, string>();
  for (let i = 0; i < profileLines.length; i++) {
    const header = projectHeader(profileLines[i]);
    if (!header) continue;
    const end = nextTable(profileLines, i + 1);
    let trustLine = -1;
    for (let j = i + 1; j < end; j++) {
      if (!trustAssignment(profileLines[j])) continue;
      if (trustLine !== -1) return null;
      trustLine = j;
    }
    if (trustLine !== -1 && trustedAssignment(profileLines[trustLine])) {
      if (entries.has(header)) return null;
      entries.set(header, 'trust_level = "trusted"');
    }
    i = end - 1;
  }
  if (entries.size === 0) return config;

  const lines = config.split(/\r?\n/);
  const newline = config.includes("\r\n") ? "\r\n" : "\n";
  let changed = false;
  for (const [header, assignment] of entries) {
    let headerIndex = -1;
    for (let i = 0; i < lines.length; i++) {
      if (projectHeader(lines[i]) === header) {
        if (headerIndex !== -1) return null;
        headerIndex = i;
      }
    }
    if (headerIndex === -1) {
      if (!config) lines.length = 0;
      else {
        let trailingBlankLines = 0;
        for (let i = lines.length - 1; i >= 0 && lines[i] === ""; i--) trailingBlankLines++;
        if (trailingBlankLines >= 2) lines.pop();
        else lines.push("");
      }
      lines.push(header, assignment);
      changed = true;
      continue;
    }
    const end = nextTable(lines, headerIndex + 1);
    let trustLine = -1;
    for (let i = headerIndex + 1; i < end; i++) {
      if (!trustAssignment(lines[i])) continue;
      if (trustLine !== -1) return null;
      trustLine = i;
    }
    if (trustLine === -1) {
      lines.splice(end, 0, assignment);
      changed = true;
    } else if (!trustedAssignment(lines[trustLine])) {
      const updated = replaceTrustAssignment(lines[trustLine]);
      if (updated === null) return null;
      lines[trustLine] = updated;
      changed = true;
    }
  }
  if (!changed) return config;
  const merged = lines.join("\n");
  return newline === "\r\n" ? merged.replace(/\n/g, "\r\n") : merged;
}

export function carryCodexProjectTrust(
  host: ProjectTrustCarryHost,
  profilePath: string,
  configPath: string,
): boolean {
  const lockId = host.fs.lockFile(configPath + ".project-trust.lock");
  if (lockId === null) return false;
  try {
    const profile = host.fs.readFile(profilePath);
    if (profile === null) return false;
    const config = host.fs.fileExists(configPath) ? host.fs.readFile(configPath) : "";
    if (config === null) return false;
    const merged = mergeTrustedProjectEntries(profile, config);
    if (merged === null) return false;
    return merged === config || host.fs.writeFileAtomic(configPath, merged);
  } finally {
    host.fs.unlockFile(lockId);
  }
}
