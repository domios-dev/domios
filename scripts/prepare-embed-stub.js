// Create an empty placeholder for the codeterm-embed sidecar binary so
// `tauri_build::build()` can validate `externalBin` before the real
// binary is compiled. Without this, the very first cargo build that
// runs the codeterm crate's build script on a fresh CI checkout fails
// with "resource path bundled-embed/codeterm-embed-<triple> doesn't
// exist". The real binary is dropped here later by bundle-embed.js.
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const isWindows = process.platform === "win32";
const exeExt = isWindows ? ".exe" : "";

const triple = execSync("rustc -vV", { encoding: "utf8" })
  .split("\n")
  .find((l) => l.startsWith("host:"))
  .split(":")[1]
  .trim();

const destDir = path.join("src-tauri", "bundled-embed");
const dest = path.join(destDir, `codeterm-embed-${triple}${exeExt}`);

fs.mkdirSync(destDir, { recursive: true });
if (!fs.existsSync(dest)) {
  fs.writeFileSync(dest, "");
  console.log(`codeterm-embed stub created: ${dest}`);
} else {
  console.log(`codeterm-embed stub already present: ${dest}`);
}
