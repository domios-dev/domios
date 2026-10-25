import fs from 'node:fs';
import path from 'node:path';

export function verifyReceipts(receipts) {
  if (receipts.filter((r) => r.mode === 'full').length !== 1) throw new Error('Expected exactly one full-suite receipt');
  if (receipts.filter((r) => r.mode === 'embeddings').length !== 1) throw new Error('Expected exactly one embeddings receipt');
  if (receipts.find(r => r.mode === 'full').os !== 'Linux') throw new Error('Full suite must cover Linux');
  for (const os of ['macOS', 'Windows']) {
    if (receipts.filter((r) => r.mode === 'platform' && r.os === os).length !== 1) throw new Error(`Expected one ${os} platform receipt`);
  }
  if (new Set(receipts.map((r) => r.sha)).size !== 1) throw new Error('Mixed source SHAs');
  for (const receipt of receipts) {
    if (receipt.exit !== 0) throw new Error(`Failed Rust ${receipt.mode} tests on ${receipt.runner}`);
    if (!(receipt.executed > 0)) throw new Error(`No executed ${receipt.mode} tests on ${receipt.runner}`);
  }
  return '| Suite | Runner | OS | Arch | Executed | Duration |\n| --- | --- | --- | --- | ---: | ---: |\n' +
    receipts.map((r) => `| ${r.mode} | ${r.runner} | ${r.os} | ${r.arch} | ${r.executed} | ${r.seconds.toFixed(1)} s |`).join('\n');
}


export function readReceipts(directory) {
  const selected = new Map();
  for (const name of fs.readdirSync(directory).filter(file => file.endsWith('.json'))) {
    const receipt = JSON.parse(fs.readFileSync(path.join(directory, name), 'utf8'));
    const producer = [receipt.mode, receipt.os, receipt.arch || ''].join('/');
    const attempt = receipt.run_attempt || 1;
    if (!Number.isSafeInteger(attempt) || attempt < 1) throw new Error('Invalid Rust receipt attempt: ' + name);
    const previous = selected.get(producer);
    if (!previous || attempt > (previous.run_attempt || 1)) selected.set(producer, receipt);
    else if (attempt === (previous.run_attempt || 1) && JSON.stringify(previous) !== JSON.stringify(receipt)) {
      throw new Error('Conflicting Rust receipts for ' + producer + ' attempt ' + attempt);
    }
  }
  return [...selected.values()];
}
