// Create an empty placeholder for the bundled CLI resource so
// `tauri_build::build()` can validate `bundle.resources` before CI
// lint/unit jobs build the real CLI. Release/dev prep replaces this
// with the actual binary via bundle-cli.js.
const fs = require("fs");
const path = require("path");

const isWindows = process.platform === "win32";
const binName = isWindows ? "codeterm.exe" : "codeterm";
const destDir = path.join("src-tauri", "bundled-cli");
const dest = path.join(destDir, binName);

fs.mkdirSync(destDir, { recursive: true });
if (!fs.existsSync(dest)) {
  fs.writeFileSync(dest, "");
  console.log(`codeterm CLI stub created: ${dest}`);
} else {
  console.log(`codeterm CLI stub already present: ${dest}`);
}
