import { readFileSync } from "node:fs";

const rootVersion = JSON.parse(readFileSync("package.json", "utf8")).version;

function lockedVersion(path, packageName) {
  const lock = readFileSync(path, "utf8");
  const match = lock.match(
    new RegExp(`\\[\\[package\\]\\]\\r?\\nname = "${packageName}"\\r?\\nversion = "([^"]+)"`),
  );

  if (!match) throw new Error(`Could not find ${packageName} in ${path}`);
  return match[1];
}

const locks = [
  ["src-tauri/Cargo.lock", "codeterm"],
  ["cli/Cargo.lock", "codeterm-cli"],
];

const stale = locks
  .map(([path, name]) => ({ path, name, version: lockedVersion(path, name) }))
  .filter(({ version }) => version !== rootVersion);

if (stale.length > 0) {
  const details = stale.map(({ path, name, version }) => `${path} (${name}=${version})`).join(", ");
  throw new Error(
    `Release lock version mismatch: package.json=${rootVersion}; ${details}. ` +
      "Run the canonical release preflight, commit both lockfiles, then retry the production build.",
  );
}

console.log(`Release lockfiles match version ${rootVersion}`);
