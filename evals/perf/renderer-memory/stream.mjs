// Synthetic busy-agent output: colored tool-call-like lines at a steady rate.
// Usage: node stream.mjs [linesPerTick=4] [tickMs=100]
const linesPerTick = Number(process.argv[2] ?? 4);
const tickMs = Number(process.argv[3] ?? 100);
const verbs = ["Read", "Edit", "Bash", "Grep", "Write", "Glob"];
let line = 0;
setInterval(() => {
  let out = "";
  for (let index = 0; index < linesPerTick; index += 1) {
    line += 1;
    const verb = verbs[line % verbs.length];
    out += `\x1b[36m● ${verb}\x1b[0m(src/components/Terminal/file_${line % 97}.tsx) \x1b[90m⎿ ${line} lines, ${"x".repeat(40 + (line % 60))}\x1b[0m\r\n`;
  }
  process.stdout.write(out);
}, tickMs);
