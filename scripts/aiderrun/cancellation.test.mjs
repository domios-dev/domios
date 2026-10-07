import assert from 'node:assert/strict';
import { run } from './process.mjs';
const result = await run([process.execPath, '-e', 'while (true) {}'], process.cwd(), 3000);
assert.equal(result.timeout, true);
assert.equal(result.contained, true);
assert.notEqual(result.code, 0);
console.log(JSON.stringify(result));
