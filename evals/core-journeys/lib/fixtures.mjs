const WORDS = ["amber", "basalt", "cobalt", "delta", "ember", "fjord", "garnet", "harbor", "indigo", "juniper"];

export function runId(now = Date.now(), entropy = Math.random()) {
  return `${now.toString(36)}${Math.floor(entropy * 36 ** 3).toString(36).padStart(3, "0")}`.toUpperCase();
}

export function marker(run, provider, suffix) {
  return `CJ${run}${provider}${suffix}`.toUpperCase().replace(/[^A-Z0-9]/g, "");
}

export function historyFiller(bytes) {
  const lines = [];
  let size = 0;
  for (let index = 0; size < bytes; index += 1) {
    const words = Array.from({ length: 12 }, (_, offset) => WORDS[(index + offset * 7) % WORDS.length]);
    const line = `${String(index).padStart(6, "0")} ${words.join(" ")}\n`;
    lines.push(line);
    size += line.length;
  }
  return lines.join("").slice(0, bytes);
}

export function workerBrief(run) {
  return [
    `Release journey probe ${run}. This task is read-only: change no files, run no builds or tests, and spawn nothing.`,
    "After acknowledging, immediately send your report with status in_review, CHANGES none, TESTS RUN none.",
    "If the task is bounced back to you, report again the same way with status in_review.",
  ].join("\n");
}
