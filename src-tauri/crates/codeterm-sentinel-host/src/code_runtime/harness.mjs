import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';

const root = new URL('.', import.meta.url);
const incoming = JSON.parse(fs.readFileSync(0, 'utf8'));
const output = JSON.parse(fs.readFileSync(new URL('output.json', root), 'utf8'));
const write = process.stderr.write.bind(process.stderr);
let denied = false;
function observe(error) {
    if (error && ['EPERM', 'EACCES'].includes(error.code)) {
        denied = true;
        write('DOMIOS_SANDBOX_DENIED\n');
    }
}
process.on('uncaughtExceptionMonitor', observe);
for (const name of ['openSync', 'readFileSync', 'writeFileSync', 'unlinkSync', 'renameSync', 'mkdirSync']) {
    const original = fs[name];
    fs[name] = function (...args) {
        try { return original.apply(this, args); }
        catch (error) { observe(error); throw error; }
    };
}
syncBuiltinESMExports();
const stdout = process.stdout.write.bind(process.stdout);
process.stdout.write = process.stderr.write.bind(process.stderr);
try {
    const body = await import(new URL('body.mts', root).href);
    if (typeof body.default !== 'function') {
        write('DOMIOS_CONTRACT_VIOLATION\n');
        process.exitCode = 78;
    } else {
        output.value = await body.default(incoming.value);
        output.text = '';
        if (denied) process.exitCode = 77;
        else stdout(JSON.stringify(output));
    }
} catch (error) {
    observe(error);
    throw error;
}
