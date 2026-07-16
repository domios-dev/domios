// Copy codeterm-embed sidecar binary into bundled-cli/ with Tauri's
// target-triple suffix so it can be referenced as externalBin and land
// next to codeterm.exe in the installed app.
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const isWindows = process.platform === "win32";
const isMacOS = process.platform === "darwin";
const exeExt = isWindows ? ".exe" : "";
const repoRoot = path.resolve(__dirname, "..");

// CARGO_BUILD_TARGET (cross-builds) wins over host triple so the suffix matches Tauri's lookup.
const buildTarget = process.env.CARGO_BUILD_TARGET || "";
const triple = buildTarget || execSync("rustc -vV", { encoding: "utf8" })
  .split("\n")
  .find((l) => l.startsWith("host:"))
  .split(":")[1]
  .trim();

function resolveEmbedReleaseDir(env = process.env, root = repoRoot) {
  const targetRoot = env.CARGO_TARGET_DIR
    ? path.resolve(env.CARGO_TARGET_DIR)
    : path.join(root, "src-tauri", "target");
  const target = env.CARGO_BUILD_TARGET || "";
  return target ? path.join(targetRoot, target, "release") : path.join(targetRoot, "release");
}

function resolveEmbedSource(env = process.env, root = repoRoot, binary = `codeterm-embed${exeExt}`) {
  return path.join(resolveEmbedReleaseDir(env, root), binary);
}

function bundleEmbed(env = process.env, root = repoRoot) {
  const buildTriple = env.CARGO_BUILD_TARGET || triple;
  const src = resolveEmbedSource(env, root);
  const destDir = path.join(root, "src-tauri", "bundled-embed");
  const dest = path.join(destDir, `codeterm-embed-${buildTriple}${exeExt}`);

  fs.mkdirSync(destDir, { recursive: true });

  if (!fs.existsSync(src)) {
    console.error(`codeterm-embed binary not found: ${src}`);
    process.exit(1);
  }
  if (fs.statSync(src).size === 0) {
    console.error(`codeterm-embed binary is an empty stub, refusing to bundle: ${src}`);
    process.exit(1);
  }

  fs.copyFileSync(src, dest);
  console.log(`codeterm-embed bundled: ${src} -> ${dest}`);

  if (isMacOS) {
    // Dev/local bundles use ad-hoc signing; release builds require Developer ID.
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
        console.log("codeterm-embed signed for notarization");
      } catch (e) {
        console.error("Failed to codesign codeterm-embed:", e.message);
        process.exit(1);
      }
    }
  }
}

if (require.main === module) {
  bundleEmbed();
} else {
  module.exports = {
    bundleEmbed,
    resolveEmbedReleaseDir,
    resolveEmbedSource,
  };
}
