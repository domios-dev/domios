import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn,execFileSync} from 'node:child_process';
import {once} from 'node:events';
import {randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {atomic,readJson,waitFile,request,shellCommand} from './io.mjs';

test('owned pipe transports exact batches, rejects replay, and observes real exit',async () => {
  const out = fs.mkdtempSync(path.join(os.tmpdir(),'soak-emitter-')), run = randomUUID(), signal = AbortSignal.timeout(20_000);
  const config = {version:1,fixtureOnly:true,run,token:randomUUID(),
    endpoint:process.platform === 'win32'?`\\\\.\\pipe\\soak-test-${run}`:path.join(out,'emitter.sock'),
    ready:path.join(out,'ready.json'),exit:path.join(out,'exit.json')};
  const configFile = path.join(out,'config.json'); atomic(configFile,config);
  const outputFile = path.join(out,'output.txt'), output = fs.openSync(outputFile,'w');
  const child = spawn(process.execPath,[fileURLToPath(new URL('./emitter.mjs',import.meta.url)),'supervise',configFile],{stdio:['ignore',output,output],signal});
  fs.closeSync(output);
  const exited = once(child,'exit');
  const message = (op,extra={}) => ({version:1,run,token:config.token,op,...extra});
  try {
    const ready = await waitFile(config.ready,signal); assert.equal(ready.run,run);
    assert.equal((await request(config.endpoint,message('status'),signal)).batch,0);
    await assert.rejects(request(config.endpoint,{...message('batch',{batch:1,lines:1}),token:'foreign'},signal));
    assert.equal((await request(config.endpoint,message('batch',{batch:1,lines:2}),signal)).batch,1);
    await assert.rejects(request(config.endpoint,message('batch',{batch:1,lines:2}),signal));
    assert.equal((await request(config.endpoint,message('batch',{batch:2,lines:1}),signal)).batch,2);
    assert.equal((await request(config.endpoint,message('stop'),signal)).batch,2);
    const exit = await waitFile(config.exit,signal); assert.equal(exit.code,0); assert.equal(exit.pid,ready.pid);
    assert.equal(exit.observed,'child exit event'); assert.deepEqual(await exited,[0,null]);
    const text = fs.readFileSync(outputFile,'utf8');
    assert.equal(text.split('\r\n').filter(Boolean).length,3);
    assert.ok(text.includes(`SOAK ${run.slice(0,8)} 1:1`)); assert.ok(text.includes(`SOAK ${run.slice(0,8)} 2:0`));
    assert.equal(readJson(configFile).run,run);
  } finally {
    if (child.exitCode === null) { try { await request(config.endpoint,message('stop'),signal); } catch {} await exited.catch(() => {}); }
    fs.rmSync(out,{recursive:true,force:true});
  }
});
test('file readiness accepts an already published atomic receipt',async () => {
  const out = fs.mkdtempSync(path.join(os.tmpdir(),'soak-receipt-'));
  try { const file = path.join(out,'receipt.json'); atomic(file,{batch:9}); assert.deepEqual(await waitFile(file,AbortSignal.timeout(1000)),{batch:9}); }
  finally { fs.rmSync(out,{recursive:true,force:true}); }
});
test('shell command preserves literal arguments through the platform shell',() => {
  const literal = "literal $value `code` 'quotes' and spaces";
  const command = shellCommand([process.execPath,'-e','process.stdout.write(process.argv[1])',literal]);
  if (process.platform === 'win32') {
    const decoded = Buffer.from(command.split(' ').at(-1),'base64').toString('utf16le');
    assert.ok(decoded.includes("'literal $value `code` ''quotes'' and spaces'"));
    assert.equal(execFileSync('powershell.exe',command.split(' ').slice(1),{encoding:'utf8',timeout:5000}),literal);
  } else assert.equal(execFileSync('/bin/sh',['-c',command],{encoding:'utf8',timeout:5000}),literal);
});
