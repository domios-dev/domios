import {
  normalizePluginActivityEvent,
  reducePluginActivity,
  resolvePluginActivityDisplay,
  type PluginActivityDisplayMap,
} from "./usePluginActivity";

let passed = 0;
let failed = 0;

function assert(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

const start = {
  id: "activity-1",
  pluginId: "transcriber",
  kind: "bootstrap",
  label: "Installing ffmpeg...",
  status: "running",
  startedAt: 100,
  updatedAt: 100,
};

const step = {
  ...start,
  label: "Downloading model...",
  percent: 42,
  updatedAt: 120,
  ignoredByReducer: true,
};

const done = {
  ...start,
  label: "Done",
  status: "done",
  updatedAt: 140,
};

console.log("plugin activity reducer:");

let activities = reducePluginActivity(new Map(), start);
assert("desktop start payload is keyed by plugin id", activities.get("transcriber")?.label === "Installing ffmpeg...");

activities = reducePluginActivity(activities, step);
assert("desktop step payload updates the existing activity", activities.get("transcriber")?.label === "Downloading model...");
assert("desktop step payload preserves percent", activities.get("transcriber")?.percent === 42);

activities = reducePluginActivity(activities, done);
assert("desktop done payload removes the active plugin", !activities.has("transcriber"));

const webStart = {
  event: "plugin_activity",
  ...start,
  id: "activity-2",
  pluginId: "lmstudio",
  label: "Installing plugin...",
  extraWebField: { ignored: true },
};
activities = reducePluginActivity(activities, webStart);
assert("web SSE payload ignores event tag and unknown fields", activities.get("lmstudio")?.id === "activity-2");

const webError = {
  ...webStart,
  event: "plugin_activity",
  status: "error",
  error: "download failed",
  updatedAt: 160,
};
activities = reducePluginActivity(activities, webError);
assert("web error payload removes the active plugin", !activities.has("lmstudio"));

assert("malformed payload is ignored", normalizePluginActivityEvent({ event: "plugin_activity", pluginId: "bad" }) === null);

const displayMap: PluginActivityDisplayMap = new Map([
  ["transcriber", { displayName: "Transcriber", iconSvg: "<svg />" }],
]);
assert(
  "known plugin id resolves to plugin display name",
  resolvePluginActivityDisplay("transcriber", displayMap).displayName === "Transcriber",
);
assert(
  "unknown plugin id falls back to plugin id",
  resolvePluginActivityDisplay("missing-plugin", displayMap).displayName === "missing-plugin",
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
