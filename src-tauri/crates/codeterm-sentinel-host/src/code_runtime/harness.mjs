import fs from 'node:fs';

const root = new URL('.', import.meta.url);
const incoming = JSON.parse(fs.readFileSync(0, 'utf8'));
const output = JSON.parse(fs.readFileSync(new URL('output.json', root), 'utf8'));
const stdout = process.stdout.write.bind(process.stdout);
process.stdout.write = process.stderr.write.bind(process.stderr);
const body = await import(new URL('body.mts', root).href);
if (typeof body.default !== 'function') {
    process.stderr.write('DOMIOS_CONTRACT_VIOLATION\n');
    process.exitCode = 78;
} else {
    output.value = await body.default(incoming.value);
    output.text = '';
    stdout(JSON.stringify(output));
}
