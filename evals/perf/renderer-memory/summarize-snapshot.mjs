// Aggregate a .heapsnapshot by constructor: count and self size, top N.
// Usage: node summarize-snapshot.mjs a.heapsnapshot [b.heapsnapshot] [--top 30]
// With two snapshots it prints the per-constructor growth from a to b.
import { readFileSync } from "node:fs";

const argv = process.argv.slice(2);
const topIndex = argv.indexOf("--top");
const top = topIndex >= 0 ? Number(argv[topIndex + 1]) : 30;
const files = argv.filter((arg, index) => !arg.startsWith("--") && index !== topIndex + 1);

function aggregate(path) {
  const snapshot = JSON.parse(readFileSync(path, "utf8"));
  const fields = snapshot.snapshot.meta.node_fields;
  const types = snapshot.snapshot.meta.node_types[0];
  const stride = fields.length;
  const typeAt = fields.indexOf("type");
  const nameAt = fields.indexOf("name");
  const sizeAt = fields.indexOf("self_size");
  const detachedAt = fields.indexOf("detachedness");
  const byName = new Map();
  let total = 0;
  let detachedNodes = 0;
  const nodes = snapshot.nodes;
  for (let index = 0; index < nodes.length; index += stride) {
    const type = types[nodes[index + typeAt]];
    const raw = snapshot.strings[nodes[index + nameAt]];
    const name = type === "object" || type === "native" || type === "closure" ? raw : `(${type})`;
    const size = nodes[index + sizeAt];
    total += size;
    if (detachedAt >= 0 && nodes[index + detachedAt] === 2) detachedNodes += 1;
    const entry = byName.get(name) ?? { count: 0, bytes: 0 };
    entry.count += 1;
    entry.bytes += size;
    byName.set(name, entry);
  }
  return { byName, total, detachedNodes };
}

const mb = (bytes) => (bytes / 1048576).toFixed(2);
const [first, second] = files.map(aggregate);
if (!second) {
  console.log(`total self ${mb(first.total)} MB, detached nodes ${first.detachedNodes}`);
  for (const [name, { count, bytes }] of [...first.byName].sort((a, b) => b[1].bytes - a[1].bytes).slice(0, top)) {
    console.log(`${mb(bytes).padStart(9)} MB ${String(count).padStart(9)}  ${name.slice(0, 80)}`);
  }
} else {
  console.log(`total self ${mb(first.total)} -> ${mb(second.total)} MB, detached nodes ${first.detachedNodes} -> ${second.detachedNodes}`);
  const names = new Set([...first.byName.keys(), ...second.byName.keys()]);
  const rows = [...names].map((name) => {
    const a = first.byName.get(name) ?? { count: 0, bytes: 0 };
    const b = second.byName.get(name) ?? { count: 0, bytes: 0 };
    return { name, bytes: b.bytes - a.bytes, count: b.count - a.count };
  });
  for (const row of rows.sort((a, b) => b.bytes - a.bytes).slice(0, top)) {
    console.log(`${mb(row.bytes).padStart(9)} MB ${String(row.count).padStart(9)}  ${row.name.slice(0, 80)}`);
  }
}
