// Copy CLI binary to src-tauri/bundled-cli/ for Tauri resource bundling.
// On macOS, codesign with hardened runtime for notarization.
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const isWindows = process.platform === "win32";
const isMacOS = process.platform === "darwin";
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

if (isMacOS) {
  const identity =
    process.env.APPLE_SIGNING_IDENTITY ||
    "Developer ID Application: ANDREY KOVALEV (JMMWQ9LWF7)";
  try {
    execSync(
      `codesign --force --options runtime --timestamp --sign "${identity}" "${dest}"`,
      { stdio: "inherit" }
    );
    console.log("CLI binary signed for notarization");
  } catch (e) {
    console.error("Failed to codesign CLI binary:", e.message);
    process.exit(1);
  }
}
