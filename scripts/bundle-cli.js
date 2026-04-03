// Copy CLI binary to src-tauri/bundled-cli/ for Tauri resource bundling.
const fs = require("fs");
const path = require("path");

const isWindows = process.platform === "win32";
const binName = isWindows ? "codeterm.exe" : "codeterm";
const src = path.join("cli", "target", "release", binName);
const destDir = path.join("src-tauri", "bundled-cli");
const dest = path.join(destDir, binName);

fs.mkdirSync(destDir, { recursive: true });

if (!fs.existsSync(src)) {
  console.error(`CLI binary not found: ${src}`);
  process.exit(1);
}

fs.copyFileSync(src, dest);
console.log(`CLI bundled: ${src} -> ${dest}`);
