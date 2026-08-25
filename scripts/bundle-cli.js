// Copy CLI binary to src-tauri/bundled-cli/ for Tauri resource bundling.
// On macOS, codesign with hardened runtime for notarization.
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const isWindows = process.platform === "win32";
const isMacOS = process.platform === "darwin";
const repoRoot = path.resolve(__dirname, "..");
const binName = isWindows ? "codeterm.exe" : "codeterm";

// One name per platform, whether or not CARGO_BUILD_TARGET is set. A local build
// used to stage `codeterm.exe` while CI staged `codeterm-<triple>.exe`, so an app
// installed from CI kept a stale triple-named resource that the startup lookup
// prefers — the fresh CLI was bundled and then permanently ignored, which is how
// a local install could leave every `codeterm` command talking an old protocol.
function hostWindowsTarget(arch = process.arch) {
  if (arch === "arm64") return "aarch64-pc-windows-msvc";
  if (arch === "ia32") return "i686-pc-windows-msvc";
  return "x86_64-pc-windows-msvc";
}

function bundledCliName(env = process.env, platform = process.platform, arch = process.arch) {
  if (platform !== "win32") return "codeterm";
  return `codeterm-${env.CARGO_BUILD_TARGET || hostWindowsTarget(arch)}.exe`;
}

function peMachineForTarget(target) {
  if (target.startsWith("x86_64-")) return 0x8664;
  if (target.startsWith("aarch64-")) return 0xaa64;
  if (target.startsWith("i686-")) return 0x014c;
  return undefined;
}

function readPeMachine(file) {
  const bytes = fs.readFileSync(file);
  if (bytes.length < 0x40 || bytes.readUInt16LE(0) !== 0x5a4d) {
    throw new Error(`CLI is not a PE executable: ${file}`);
  }
  const peOffset = bytes.readUInt32LE(0x3c);
  if (peOffset + 6 > bytes.length || bytes.readUInt32LE(peOffset) !== 0x00004550) {
    throw new Error(`CLI has an invalid PE header: ${file}`);
  }
  return bytes.readUInt16LE(peOffset + 4);
}

function assertTargetArchitecture(file, env = process.env, platform = process.platform) {
  if (platform !== "win32" || !env.CARGO_BUILD_TARGET) return;
  const expected = peMachineForTarget(env.CARGO_BUILD_TARGET);
  if (expected === undefined) {
    throw new Error(`Unsupported Windows CLI target: ${env.CARGO_BUILD_TARGET}`);
  }
  const actual = readPeMachine(file);
  if (actual !== expected) {
    throw new Error(
      `CLI architecture mismatch for ${env.CARGO_BUILD_TARGET}: expected 0x${expected.toString(16)}, got 0x${actual.toString(16)} (${file})`
    );
  }
}

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
  const dest = path.join(destDir, bundledCliName());

  fs.mkdirSync(destDir, { recursive: true });

  if (!fs.existsSync(src)) {
    console.error(`CLI binary not found: ${src}`);
    process.exit(1);
  }

  assertTargetArchitecture(src);
  fs.copyFileSync(src, dest);
  assertTargetArchitecture(dest);
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
    bundledCliName,
    hostWindowsTarget,
    peMachineForTarget,
    assertTargetArchitecture,
  };
}
