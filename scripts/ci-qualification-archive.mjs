import { execFileSync } from 'node:child_process';
import { pythonInterpreter } from './ci-runner-paths.mjs';

export function readQualificationArchive(archive) {
  const program = "import io,json,sys,zipfile\nwith zipfile.ZipFile(io.BytesIO(sys.stdin.buffer.read())) as z:\n assert z.namelist().count('qualification.json') == 1\n assert z.getinfo('qualification.json').file_size <= 16777216\n sys.stdout.buffer.write(z.read('qualification.json'))";
  return JSON.parse(execFileSync(pythonInterpreter(process.platform, process.env), ['-c', program], {
    input: Buffer.from(archive), encoding: 'utf8', timeout: 5000, maxBuffer: 16 * 1024 * 1024,
  }));
}
