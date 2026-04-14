// Copy codeterm-embed sidecar binary into bundled-cli/ with Tauri's
// target-triple suffix so it can be referenced as externalBin and land
// next to codeterm.exe in the installed app.
const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const isWindows = process.platform === "win32";
const isMacOS = process.platform === "darwin";
const exeExt = isWindows ? ".exe" : "";

const triple = execSync("rustc -vV", { encoding: "utf8" })
  .split("\n")
  .find((l) => l.startsWith("host:"))
  .split(":")[1]
  .trim();

const src = path.join("src-tauri", "target", "release", `codeterm-embed${exeExt}`);
const destDir = path.join("src-tauri", "bundled-embed");
const dest = path.join(destDir, `codeterm-embed-${triple}${exeExt}`);

fs.mkdirSync(destDir, { recursive: true });

if (!fs.existsSync(src)) {
  console.error(`codeterm-embed binary not found: ${src}`);
  process.exit(1);
}

fs.copyFileSync(src, dest);
console.log(`codeterm-embed bundled: ${src} -> ${dest}`);

if (isMacOS) {
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
