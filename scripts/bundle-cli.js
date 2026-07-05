// Copy CLI binary to src-tauri/bundled-cli/ for Tauri resource bundling.
// On macOS, codesign with hardened runtime for notarization.
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const isWindows = process.platform === "win32";
const isMacOS = process.platform === "darwin";
const repoRoot = path.resolve(__dirname, "..");
const binName = isWindows ? "codeterm.exe" : "codeterm";

function resolveCliReleaseDir(env = process.env, root = repoRoot) {
  const targetRoot = env.CARGO_TARGET_DIR
    ? path.resolve(env.CARGO_TARGET_DIR)
    : path.join(root, "cli", "target");
  const target = env.CARGO_BUILD_TARGET || "";
  return target ? path.join(targetRoot, target, "release") : path.join(targetRoot, "release");
}

function resolveCliSource(env = process.env, root = repoRoot, binary = binName) {
  return path.join(resolveCliReleaseDir(env, root), binary);
}

function bundleCli() {
  const src = resolveCliSource();
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
    // Dev uses ad-hoc signing; release builds require the Developer ID identity.
    if (process.env.CODETERM_DEV === "1") {
      try {
        execSync(`codesign --force --sign - "${dest}"`, { stdio: "inherit" });
      } catch (e) {
        console.warn("Ad-hoc codesign failed (continuing for dev):", e.message);
      }
    } else {
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
  }
}

if (require.main === module) {
  bundleCli();
} else {
  module.exports = {
    resolveCliReleaseDir,
    resolveCliSource,
  };
}
