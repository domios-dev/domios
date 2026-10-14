#!/usr/bin/env node
// Caller: package.json script
import { relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  parsedHeader,
  requiredHeaderLabels,
  scopedSpecs,
  toPosix,
} from "../tests/playwright/helpers/spec-headers.mjs";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const suitesDir = resolve(root, "tests/playwright/suites");

function check(file) {
  const header = parsedHeader(file);
  const rel = toPosix(relative(root, file));
  if (!header) {
    return `${rel}: missing spec header`;
  }

  if (header.nonEmpty.length < 3 || header.nonEmpty.length > 8) {
    return `${rel}: spec header must be 3-8 non-empty lines`;
  }

  for (const label of requiredHeaderLabels) {
    if (!header.fields.has(label)) {
      return `${rel}: spec header missing ${label}`;
    }
  }

  return null;
}

const failures = scopedSpecs({ envName: "PW_HEADER_SCOPE", fallbackDir: suitesDir }).map(check).filter(Boolean);
if (failures.length > 0) {
  console.error("Playwright spec header violations:");
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log("Playwright spec headers OK");
