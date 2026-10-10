import { mkdir, open } from 'node:fs/promises';
import { captureStream } from './http-stream.mjs';
import { evidencePath } from './evidence.mjs';
const [root, name, peer] = process.argv.slice(2);
if (!root || !name) throw new Error('Usage: stream-live.mjs RUN_DIR ARTIFACT_NAME [EXACT_PEER_ID]');
await mkdir(root, { recursive: true });
const file = await open(evidencePath(root, name), 'wx');
try {
  await captureStream({ port: Number(process.env.CODETERM_API_PORT), token: process.env.CODETERM_API_TOKEN, peer }, async entry => file.write(JSON.stringify(entry) + '\n'), AbortSignal.timeout(30000));
} catch (error) {
  await file.write(JSON.stringify({ kind: 'capture-ended', error: error.message }) + '\n');
  process.exitCode = 1;
} finally { await file.close(); }
