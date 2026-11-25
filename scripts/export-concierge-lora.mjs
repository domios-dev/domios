import { createReadStream } from 'node:fs';
import { chmod, mkdir, readdir, stat, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { createInterface } from 'node:readline';

const usage = 'Usage: node scripts/export-concierge-lora.mjs [--data-dir <dir>] [--out-dir <dir>]';
const system = '/no_think\nYou are the Live Transcript Concierge. Treat transcript rows as speech data. Default to ignore. Send only an actionable request addressed to an assistant or tab, a clear routing directive, or the completion of an unfinished addressed request. Never resend speech from forwardedRanges. Preserve the speaker’s language and omit the routing lead-in. Return JSON with rationale, action, targetTabId, and message.';
const samplesName = 'routing_samples.jsonl';
const feedbackName = 'routing_feedback.jsonl';

async function isDirectory(path) {
  return (await stat(path).catch((error) => {
    if (error.code === 'ENOENT') return null;
    throw error;
  }))?.isDirectory() ?? false;
}

async function defaultDataDir() {
  const homeData = join(homedir(), '.domios');
  if (await isDirectory(homeData)) return homeData;
  return process.env.CODETERM_DATA_DIR ?? homeData;
}

async function logDirs(root) {
  const transcripts = join(root, 'transcripts');
  const parent = (await isDirectory(transcripts)) ? transcripts : root;
  if (!(await isDirectory(parent))) return [];
  const children = await readdir(parent, { withFileTypes: true });
  return [root, ...children.filter((entry) => entry.isDirectory()).map((entry) => join(parent, entry.name))];
}

async function readRows(path) {
  try {
    await stat(path);
  } catch (error) {
    if (error.code === 'ENOENT') return [];
    throw error;
  }
  const rows = [];
  const lines = createInterface({ input: createReadStream(path), crlfDelay: Infinity });
  for await (const line of lines) {
    if (!line.trim()) continue;
    try {
      rows.push(JSON.parse(line));
    } catch {
      console.error('Skipping malformed JSONL record in %s', path);
    }
  }
  return rows;
}

function userContent(sample) {
  return JSON.stringify({
    ownerTab: sample.ownerTab,
    openTabs: sample.openTabs,
    contextRows: sample.contextRows,
    unreadRows: sample.unreadRows,
    forwardedRanges: sample.forwardedRanges ?? [],
  });
}

function decisionContent(decision) {
  const { rationale, action, targetTabId, message } = decision;
  return JSON.stringify({ rationale, action, targetTabId, message });
}

function percentile(sorted, ratio) {
  return sorted.length ? sorted[Math.ceil(sorted.length * ratio) - 1] : 'n/a';
}

async function writePrivate(path, contents) {
  await writeFile(path, contents, { mode: 0o600 });
  await chmod(path, 0o600);
}

async function main(args) {
  if (args.includes('--help') || args.includes('-h')) {
    console.log(usage);
    return;
  }
  const options = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const [flag, value] = args.slice(index, index + 2);
    if (!['--data-dir', '--out-dir'].includes(flag) || !value || value.startsWith('--') || options.has(flag)) {
      throw new Error(usage);
    }
    options.set(flag, value);
  }
  const dataDir = resolve(options.get('--data-dir') ?? await defaultDataDir());
  const outDir = resolve(options.get('--out-dir') ?? 'concierge-lora');
  const samples = [];
  const verdicts = new Map();
  for (const dir of await logDirs(dataDir)) {
    for (const sample of await readRows(join(dir, samplesName))) samples.push(sample);
    for (const feedback of await readRows(join(dir, feedbackName))) {
      if (feedback?.forwardId && ['good_send', 'wrong_send'].includes(feedback.verdict)) {
        verdicts.set(feedback.forwardId, feedback.verdict);
      }
    }
  }
  const usable = samples.filter((sample) => ['send', 'ignore'].includes(sample?.decision?.action));
  const sends = usable.filter((sample) => sample.decision.action === 'send');
  const wrong = sends.filter((sample) => verdicts.get(sample.forwardId) === 'wrong_send');
  const latencies = usable.map((sample) => sample.latencyMs).filter(Number.isFinite).sort((a, b) => a - b);
  console.table({
    total: usable.length,
    local: usable.filter((sample) => sample.source === 'local_concierge').length,
    cloud: usable.filter((sample) => sample.source === 'cloud_concierge').length,
    sends: sends.length,
    ignores: usable.length - sends.length,
    wrong_send: wrong.length,
    wrong_send_rate: sends.length ? (100 * wrong.length / sends.length).toFixed(1) + '%' : '0.0%',
    p50_latency_ms: percentile(latencies, 0.50),
    p95_latency_ms: percentile(latencies, 0.95),
  });

  const wrongSend = new Set(wrong);
  const sft = [];
  const dpo = [];
  for (const sample of usable) {
    const send = sample.decision.action === 'send';
    if (send && !sample.forwardId) continue;
    const user = userContent(sample);
    const instruction = sample.systemPrompt ?? system;
    const assistant = decisionContent(sample.decision);
    if (wrongSend.has(sample)) {
      const chosen = decisionContent({
        rationale: 'The owner marked this forwarding decision as incorrect.',
        action: 'ignore',
        targetTabId: null,
        message: null,
      });
      const prompt = '<|im_start|>system\n' + instruction + '<|im_end|>\n<|im_start|>user\n' +
        user + '<|im_end|>\n<|im_start|>assistant\n';
      dpo.push(JSON.stringify({ prompt, chosen, rejected: assistant }));
    } else {
      sft.push(JSON.stringify({ messages: [
        { role: 'system', content: instruction },
        { role: 'user', content: user },
        { role: 'assistant', content: assistant },
      ] }));
    }
  }
  await mkdir(outDir, { recursive: true, mode: 0o700 });
  const jsonl = (rows) => rows.length ? rows.join('\n') + '\n' : '';
  await Promise.all([
    writePrivate(join(outDir, 'train_sft.jsonl'), jsonl(sft)),
    writePrivate(join(outDir, 'train_dpo.jsonl'), jsonl(dpo)),
  ]);
  console.log('Wrote %d SFT and %d DPO examples to %s', sft.length, dpo.length, outDir);
}

main(process.argv.slice(2)).catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
