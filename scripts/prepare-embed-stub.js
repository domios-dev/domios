// Keep a placeholder so `tauri_build::build()` validates `externalBin` before the real
// sidecar is compiled; `bundle-embed.js` replaces it later.
const fs = require("fs");
const os = require("os");
const path = require("path");
const { execFileSync } = require("child_process");

const isWindows = process.platform === "win32";
const exeExt = isWindows ? ".exe" : "";

const cargoHome = process.env.CARGO_HOME || path.join(os.homedir(), ".cargo");
const cargoRustc = path.join(cargoHome, "bin", `rustc${exeExt}`);
const rustc = process.env.RUSTC || (fs.existsSync(cargoRustc) ? cargoRustc : "rustc");
const triple = execFileSync(rustc, ["-vV"], { encoding: "utf8" })
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
