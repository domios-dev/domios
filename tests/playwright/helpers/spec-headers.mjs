import { readdirSync, readFileSync, statSync } from "node:fs";
import { delimiter, isAbsolute, relative, resolve, sep } from "node:path";

export const requiredHeaderLabels = ["WHAT:", "WHY:", "ORACLE:"];

export function toPosix(path) {
  return path.split(sep).join("/");
}

export function specs(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = resolve(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...specs(path));
    } else if (entry.isFile() && entry.name.endsWith(".spec.ts")) {
      out.push(path);
    }
  }
  return out.sort();
}

export function scopedSpecs({ envName, fallbackDir, cwd = process.cwd() }) {
  const scope = process.env[envName];
  if (!scope) {
    return specs(fallbackDir);
  }

  return scope
    .split(delimiter)
    .filter(Boolean)
    .flatMap((entry) => {
      const path = isAbsolute(entry) ? entry : resolve(cwd, entry);
      const stat = statSync(path);
      if (stat.isDirectory()) {
        return specs(path);
      }
      return stat.isFile() && path.endsWith(".spec.ts") ? [path] : [];
    })
    .sort();
}

function firstMeaningfulLine(lines) {
  return lines.findIndex((line) => line.trim() !== "" && !line.trim().startsWith("// @"));
}

export function headerLines(text) {
  const lines = text.split(/\r?\n/);
  const start = firstMeaningfulLine(lines);
  if (start < 0) {
    return null;
  }

  const first = lines[start].trim();
  if (first === "/**") {
    const end = lines.findIndex((line, index) => index > start && line.trim() === "*/");
    return end < 0 ? null : lines.slice(start + 1, end).map((line) => line.replace(/^\s*\*\s?/, "").trim());
  }

  if (first.startsWith("//")) {
    const out = [];
    for (let i = start; i < lines.length; i += 1) {
      const trimmed = lines[i].trim();
      if (!trimmed.startsWith("//")) {
        break;
      }
      out.push(trimmed.replace(/^\/\/\s?/, "").trim());
    }
    return out;
  }

  return null;
}

export function parsedHeader(file) {
  const lines = headerLines(readFileSync(file, "utf8"));
  if (!lines) {
    return null;
  }

  const fields = new Map();
  const nonEmpty = lines.filter(Boolean);
  for (const line of nonEmpty) {
    const match = /^([A-Z][A-Z0-9_-]*):\s*(.+)$/.exec(line);
    if (match) {
      fields.set(`${match[1]}:`, match[2].trim());
    }
  }

  return { lines, nonEmpty, fields };
}

export function surfaceFromSpec(file, root) {
  const parsed = parsedHeader(file);
  const explicit = parsed?.fields.get("SURFACE:");
  if (explicit) {
    return explicit;
  }

  const rel = toPosix(relative(resolve(root, "tests/playwright/suites"), file));
  return rel.split("/")[0] ?? "unknown";
}
