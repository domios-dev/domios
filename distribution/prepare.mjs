import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { lstat, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const templates = path.dirname(fileURLToPath(import.meta.url));
const targets = [
  ['windows-x86_64', 'x64-setup.exe', 'WINDOWS_X64'],
  ['windows-aarch64', 'arm64-setup.exe', 'WINDOWS_ARM64'],
  ['darwin-x86_64', 'x64.dmg', 'MACOS_X64'],
  ['darwin-aarch64', 'aarch64.dmg', 'MACOS_ARM64'],
];

async function fingerprint(file) {
  const stat = await lstat(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size === 0) throw new Error('Missing nonempty regular asset: ' + file);
  const hash = createHash('sha256');
  for await (const bytes of createReadStream(file)) hash.update(bytes);
  return { size: stat.size, sha256: hash.digest('hex') };
}

export async function prepare({ sourceSha, receiptFile, inputDirectory, outputDirectory }) {
  const receipt = JSON.parse(await readFile(receiptFile, 'utf8'));
  if (!/^[a-f0-9]{40}$/.test(sourceSha ?? '') || receipt.schema !== 1 || receipt.mode !== 'local_dry_run' ||
      receipt.source_sha !== sourceSha || !/^v\d+\.\d+\.\d+$/.test(receipt.tag ?? '') || !Array.isArray(receipt.evidence)) {
    throw new Error('Expected the accepted local dry-run receipt for the pinned source SHA');
  }
  const version = receipt.tag.slice(1);
  const replacements = { VERSION: version };
  const assets = [];
  for (const [platform, suffix, token] of targets) {
    const platforms = receipt.evidence.filter(entry => entry.platform === platform);
    if (platforms.length !== 1 || platforms[0].schema !== 1 || platforms[0].tag !== receipt.tag ||
        platforms[0].source_sha !== sourceSha || !Array.isArray(platforms[0].files)) {
      throw new Error('Missing, duplicate or foreign platform receipt: ' + platform);
    }
    const name = `Domios_${version}_${suffix}`;
    const matches = platforms[0].files.filter(file => file.name === name);
    if (matches.length !== 1) throw new Error('Expected one desktop asset receipt: ' + name);
    const actual = await fingerprint(path.join(inputDirectory, platform, name));
    if (matches[0].size !== actual.size || matches[0].sha256 !== actual.sha256) throw new Error('Desktop asset digest mismatch: ' + name);
    replacements[`SHA256_${token}`] = actual.sha256;
    assets.push({ platform, name, ...actual, url: `https://github.com/domios-dev/domios/releases/download/${receipt.tag}/${name}` });
  }
  const outputs = [];
  for (const relative of ['scoop/bucket/domios.json', 'homebrew/Casks/domios.rb']) {
    const template = await readFile(path.join(templates, relative), 'utf8');
    const content = template.replace(/@([A-Z0-9_]+)@/g, (_, token) => {
      if (!replacements[token]) throw new Error('Unknown distribution placeholder: ' + token);
      return replacements[token];
    });
    outputs.push([relative, content]);
  }
  await mkdir(outputDirectory, { recursive: true });
  if ((await readdir(outputDirectory)).length !== 0) throw new Error('Distribution output directory must be empty');
  for (const [relative, content] of outputs) {
    const destination = path.join(outputDirectory, relative);
    await mkdir(path.dirname(destination), { recursive: true });
    await writeFile(destination, content, { flag: 'wx' });
  }
  const result = { source_sha: sourceSha, tag: receipt.tag, assets };
  await writeFile(path.join(outputDirectory, 'package-inputs.json'), JSON.stringify(result, null, 2) + '\n', { flag: 'wx' });
  return result;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [sourceSha, receiptFile, inputDirectory, outputDirectory] = process.argv.slice(2);
  try {
    if (!receiptFile || !inputDirectory || !outputDirectory || process.argv.length !== 6) {
      throw new Error('Usage: node distribution/prepare.mjs SOURCE_SHA RECEIPT_FILE RETAINED_INPUT_ROOT EMPTY_OUTPUT_DIR');
    }
    const result = await prepare({ sourceSha, receiptFile, inputDirectory, outputDirectory });
    console.log(`Prepared ${result.assets.length} desktop asset definitions for ${result.tag}; publication and native verification remain separate.`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
