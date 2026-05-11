const fs = require("fs");
const path = require("path");

const target = path.resolve(__dirname, "..", "cli", "src", "main.rs");

if (fs.existsSync(target)) {
  const now = new Date();
  fs.utimesSync(target, now, now);
} else {
  fs.closeSync(fs.openSync(target, "w"));
}
