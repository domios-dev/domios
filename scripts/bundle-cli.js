// Copy CLI binary to src-tauri/bundled-cli/ for Tauri resource bundling.
// On macOS, codesign with hardened runtime for notarization.
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const isWindows = process.platform === "win32";
const isMacOS = process.platform === "darwin";
const repoRoot = path.resolve(__dirname, "..");
const binName = isWindows ? "codeterm.exe" : "codeterm";
// Honor cross-build target so the bundled CLI matches the .app arch.
const target = process.env.CARGO_BUILD_TARGET || "";
const targetDir = target ? path.join("cli", "target", target, "release") : path.join("cli", "target", "release");
const src = path.join(repoRoot, targetDir, binName);
const destDir = path.join(repoRoot, "src-tauri", "bundled-cli");
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
