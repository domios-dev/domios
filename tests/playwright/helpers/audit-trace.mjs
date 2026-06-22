import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { basename, dirname, extname, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { inflateRawSync } from "node:zlib";

const helpersDir = fileURLToPath(new URL(".", import.meta.url));
const repoRoot = resolve(helpersDir, "../../..");
const defaultArtifactsDir = resolve(helpersDir, "../artifacts");

const actionMethods = new Set([
  "check",
  "click",
  "dblclick",
  "dispatchEvent",
  "dragTo",
  "fill",
  "focus",
  "goto",
  "hover",
  "navigate",
  "press",
  "reload",
  "selectOption",
  "setChecked",
  "setContent",
  "setInputFiles",
  "tap",
  "type",
  "uncheck",
]);

const realWaitMethods = new Set([
  "expect",
  "waitFor",
  "waitForEvent",
  "waitForFunction",
  "waitForLoadState",
  "waitForRequest",
  "waitForResponse",
  "waitForSelector",
  "waitForURL",
]);

function toPosix(path) {
  return path.split(sep).join("/");
}

function repoRelative(path) {
  const rel = toPosix(relative(repoRoot, path));
  return rel.startsWith("..") ? toPosix(path) : rel;
}

function readUInt16(buffer, offset) {
  return buffer.readUInt16LE(offset);
}

function readUInt32(buffer, offset) {
  return buffer.readUInt32LE(offset);
}

function findEndOfCentralDirectory(buffer) {
  const min = Math.max(0, buffer.length - 65_557);
  for (let offset = buffer.length - 22; offset >= min; offset -= 1) {
    if (readUInt32(buffer, offset) === 0x06054b50) return offset;
  }
  throw new Error("ZIP end of central directory not found");
}

function readZipEntries(zipPath) {
  const buffer = readFileSync(zipPath);
  const eocd = findEndOfCentralDirectory(buffer);
  const entryCount = readUInt16(buffer, eocd + 10);
  const centralOffset = readUInt32(buffer, eocd + 16);
  const entries = new Map();
  let offset = centralOffset;

  for (let i = 0; i < entryCount; i += 1) {
    if (readUInt32(buffer, offset) !== 0x02014b50) {
      throw new Error(`invalid ZIP central directory at ${offset}`);
    }
    const method = readUInt16(buffer, offset + 10);
    const compressedSize = readUInt32(buffer, offset + 20);
    const uncompressedSize = readUInt32(buffer, offset + 24);
    const fileNameLength = readUInt16(buffer, offset + 28);
    const extraLength = readUInt16(buffer, offset + 30);
    const commentLength = readUInt16(buffer, offset + 32);
    const localOffset = readUInt32(buffer, offset + 42);
    const name = buffer.subarray(offset + 46, offset + 46 + fileNameLength).toString("utf8");

    if (readUInt32(buffer, localOffset) !== 0x04034b50) {
      throw new Error(`invalid ZIP local header for ${name}`);
    }
    const localNameLength = readUInt16(buffer, localOffset + 26);
    const localExtraLength = readUInt16(buffer, localOffset + 28);
    const dataStart = localOffset + 30 + localNameLength + localExtraLength;
    const compressed = buffer.subarray(dataStart, dataStart + compressedSize);
    let bytes;
    if (method === 0) {
      bytes = compressed;
    } else if (method === 8) {
      bytes = inflateRawSync(compressed);
    } else {
      throw new Error(`unsupported ZIP compression method ${method} in ${name}`);
    }
    if (uncompressedSize !== 0 && bytes.length !== uncompressedSize) {
      throw new Error(`ZIP entry size mismatch for ${name}`);
    }
    entries.set(name, bytes);
    offset += 46 + fileNameLength + extraLength + commentLength;
  }

  return entries;
}

function parseNdjson(bytes) {
  return bytes
    .toString("utf8")
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

function findTraceZips(inputs) {
  const roots = inputs.length ? inputs.map((input) => resolve(input)) : [defaultArtifactsDir];
  const out = [];
  function visit(path) {
    if (!existsSync(path)) return;
    const stat = statSync(path);
    if (stat.isDirectory()) {
      for (const entry of readdirSync(path)) visit(resolve(path, entry));
    } else if (stat.isFile() && basename(path) === "trace.zip") {
      out.push(path);
    } else if (stat.isFile() && extname(path) === ".zip") {
      out.push(path);
    }
  }
  for (const root of roots) visit(root);
  return [...new Set(out)].sort();
}

function eventMethod(event) {
  return String(event.method ?? "").replace(/^pw:api$/, "");
}

function uniqueBeforeEvents(events) {
  const byKey = new Map();
  for (const event of events) {
    const key = event.stepId ?? event.callId ?? `${event.startTime}:${event.title}:${event.method}`;
    const existing = byKey.get(key);
    if (!existing) {
      byKey.set(key, event);
      continue;
    }
    const existingMethod = eventMethod(existing);
    const nextMethod = eventMethod(event);
    if ((!existingMethod || existingMethod === "pw:api") && nextMethod && nextMethod !== "pw:api") {
      byKey.set(key, event);
    }
  }
  return [...byKey.values()];
}

function sourceLocations(events) {
  const locations = [];
  for (const event of events) {
    for (const frame of event.stack ?? []) {
      if (frame.file && typeof frame.line === "number") {
        locations.push(`${repoRelative(frame.file)}:${frame.line}`);
      }
    }
  }
  return [...new Set(locations)].slice(0, 5);
}

function recordTitle(zipPath, events) {
  const titled = events.find((event) => typeof event.title === "string" && event.title.includes("›"));
  if (titled) return titled.title;
  return basename(dirname(zipPath)).replace(/-/g, " ");
}

function scoreFitness(metrics) {
  let score = 100;
  if (!metrics.hasAssertions) score -= 35;
  if (metrics.sleepCount > 0) score -= Math.min(45, metrics.sleepCount * 15);
  if (metrics.actionCount === 0) score -= 15;
  if (metrics.actionCount > 50) score -= 10;
  if (metrics.screenshots === 0 && metrics.screencastFrames === 0) score -= 5;
  score = Math.max(0, score);
  const verdict = score >= 80 ? "good" : score >= 60 ? "watch" : "weak";
  return { score, verdict };
}

function analyzeTraceZip(zipPath) {
  const entries = readZipEntries(zipPath);
  const traceEntries = [...entries.entries()].filter(
    ([name]) => name === "test.trace" || /(?:^|\/)\d+-trace\.trace$/.test(name) || name.endsWith("/trace.trace"),
  );
  const events = traceEntries.flatMap(([, bytes]) => parseNdjson(bytes));
  const beforeEvents = uniqueBeforeEvents(events.filter((event) => event.type === "before"));
  const methods = beforeEvents.map(eventMethod);
  const sleepCount = beforeEvents.filter((event) => {
    const method = eventMethod(event);
    const title = String(event.title ?? "");
    return method === "waitForTimeout" || /wait for timeout|waitForTimeout/i.test(title);
  }).length;
  const assertionCount = beforeEvents.filter((event) => {
    const method = eventMethod(event);
    const title = String(event.title ?? "");
    return method === "expect" || title.startsWith("Expect ");
  }).length;
  const realWaitCount = beforeEvents.filter((event) => {
    const method = eventMethod(event);
    return realWaitMethods.has(method) && method !== "waitForTimeout";
  }).length;
  const actionCount =
    beforeEvents.filter((event) => actionMethods.has(eventMethod(event))).length +
    events.filter((event) => event.type === "input").length;
  const screenshots =
    events.filter((event) =>
      (event.attachments ?? []).some((attachment) => /screenshot/i.test(String(attachment.name ?? ""))),
    ).length + [...entries.keys()].filter((name) => /\.(?:png|webp)$/i.test(name)).length;
  const screencastFrames = events.filter((event) => event.type === "screencast-frame").length;
  const fitness = scoreFitness({
    actionCount,
    hasAssertions: assertionCount > 0,
    sleepCount,
    screenshots,
    screencastFrames,
  });

  return {
    tracePath: repoRelative(zipPath),
    title: recordTitle(zipPath, events),
    sourceLocations: sourceLocations(events),
    actionCount,
    apiCallCount: beforeEvents.length,
    assertionCount,
    hasAssertions: assertionCount > 0,
    realWaitCount,
    sleepCount,
    screenshots,
    screencastFrames,
    score: fitness.score,
    verdict: fitness.verdict,
    methods: [...new Set(methods.filter(Boolean))].sort(),
  };
}

export function runTraceAudit(options = {}) {
  const tracePaths = findTraceZips(options.inputs ?? []);
  const records = [];
  const errors = [];
  for (const tracePath of tracePaths) {
    try {
      records.push(analyzeTraceZip(tracePath));
    } catch (error) {
      errors.push({
        tracePath: repoRelative(tracePath),
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }
  return {
    generatedAt: new Date().toISOString(),
    summary: {
      traces: tracePaths.length,
      records: records.length,
      errors: errors.length,
    },
    records,
    errors,
  };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const result = runTraceAudit({ inputs: process.argv.slice(2) });
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
}
