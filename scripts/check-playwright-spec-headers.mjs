#!/usr/bin/env node
import { readdirSync, readFileSync, statSync } from "node:fs";
import { delimiter, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const suitesDir = resolve(root, "tests/playwright/suites");
const required = ["WHAT:", "WHY:", "ORACLE:"];

function toPosix(path) {
  return path.split(sep).join("/");
}

function specs(dir) {
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

function scopedSpecs() {
  const scope = process.env.PW_HEADER_SCOPE;
  if (!scope) {
    return specs(suitesDir);
  }

  return scope
    .split(delimiter)
    .filter(Boolean)
    .flatMap((entry) => {
      const path = isAbsolute(entry) ? entry : resolve(process.cwd(), entry);
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

function headerLines(text) {
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

function check(file) {
  const lines = headerLines(readFileSync(file, "utf8"));
  const rel = toPosix(relative(root, file));
  if (!lines) {
    return `${rel}: missing spec header`;
  }

  const nonEmpty = lines.filter(Boolean);
  if (nonEmpty.length < 3 || nonEmpty.length > 8) {
    return `${rel}: spec header must be 3-8 non-empty lines`;
  }

  for (const label of required) {
    if (!nonEmpty.some((line) => line.startsWith(label))) {
      return `${rel}: spec header missing ${label}`;
    }
  }

  return null;
}

const failures = scopedSpecs().map(check).filter(Boolean);
if (failures.length > 0) {
  console.error("Playwright spec header violations:");
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log("Playwright spec headers OK");
