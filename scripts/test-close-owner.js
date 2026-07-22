const fs = require("fs");

const source = fs.readFileSync("src/store/multiplexer/index.tsx", "utf8");

function bodyOfConst(name) {
  const startNeedle = `const ${name} = useCallback`;
  const start = source.indexOf(startNeedle);
  if (start < 0) throw new Error(`${name} callback not found`);
  const end = source.indexOf(`\n\n  const `, start + startNeedle.length);
  return source.slice(start, end < 0 ? source.length : end);
}

for (const name of ["closePane", "closeWorkspace"]) {
  const body = bodyOfConst(name);
  if (body.includes('"kill_pty"') || body.includes("'kill_pty'")) {
    throw new Error(`${name} must not preflight kill_pty before backend close`);
  }
}

const closePane = bodyOfConst("closePane");
const closeWorkspace = bodyOfConst("closeWorkspace");

if ((closePane.match(/mux_close_pane/g) || []).length !== 1) {
  throw new Error("closePane must have exactly one backend pane close owner");
}

if ((closeWorkspace.match(/mux_close_workspace/g) || []).length !== 1) {
  throw new Error("closeWorkspace must have exactly one backend workspace close owner");
}

console.log("close owner regression checks passed");
