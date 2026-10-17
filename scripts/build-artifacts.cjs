const fs = require('node:fs');
const path = require('node:path');
const { randomUUID } = require('node:crypto');

function copyFileIfChanged(source, destination) {
  if (fs.existsSync(destination) && fs.statSync(destination).isFile()
      && fs.readFileSync(source).equals(fs.readFileSync(destination))) return false;
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  const temporary = `${destination}.${randomUUID()}.tmp`;
  try {
    fs.copyFileSync(source, temporary, fs.constants.COPYFILE_EXCL);
    fs.renameSync(temporary, destination);
  } finally {
    fs.rmSync(temporary, { force: true });
  }
  return true;
}

function files(directory, prefix = '', result = new Map()) {
  if (!fs.lstatSync(directory).isDirectory()) throw new Error(`Expected build directory: ${directory}`);
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const relative = path.join(prefix, entry.name);
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) files(full, relative, result);
    else if (entry.isFile()) result.set(relative, full);
    else throw new Error(`Expected regular build output: ${full}`);
  }
  return result;
}

function syncBuildDirectory(source, destination) {
  const inputs = files(source);
  const previous = fs.existsSync(destination) ? files(destination) : new Map();
  fs.mkdirSync(destination, { recursive: true });
  const entries = [...inputs].sort(([a], [b]) => Number(a === 'index.html') - Number(b === 'index.html'));
  for (const [relative, input] of entries) copyFileIfChanged(input, path.join(destination, relative));
  for (const [relative, old] of previous) if (!inputs.has(relative)) fs.rmSync(old);
}

module.exports = { copyFileIfChanged, syncBuildDirectory };
