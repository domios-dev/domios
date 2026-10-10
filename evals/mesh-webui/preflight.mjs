import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {gatewayToken} from './secrets.mjs';
import {validateManifest} from './model.mjs';
const m=validateManifest(JSON.parse(await readFile(process.argv[2],'utf8')));
assert.equal(m.windows.peerId,process.argv[3]);
assert.equal(process.env.B2_GO_LIVE,'yes');
for(const machine of [m.mac,m.windows]) await gatewayToken(machine);
