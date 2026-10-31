import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import { performance } from "node:perf_hooks";

export function prepareNextestMetadata({ root = process.cwd(), manifest, targets, output,
  env = process.env, run = execFileSync, log = console.error }) {
  const options = { cwd: root, env, encoding: "utf8", maxBuffer: 128 * 1024 * 1024, stdio: ["ignore", "pipe", "inherit"] };
  const timed = (phase, action) => {
    const start = performance.now();
    try { return action(); } catch (error) {
      for (const line of String(error.stdout ?? "").split(/\r?\n/)) {
        let row;
        try { row = JSON.parse(line); } catch { continue; }
        if (row?.reason === "compiler-message" && ["error", "warning"].includes(row.message?.level)) {
          log(row.message.rendered?.trimEnd() || row.message.message);
        }
      }
      throw error;
    } finally { log(`[test-loop] ${phase} ${(performance.now() - start).toFixed(1)}ms`); }
  };
  const files = run("git", ["ls-files", "--cached", "--others", "--exclude-standard", "-z"], options)
    .split("\0").filter(file => /(^|\/)(Cargo\.(toml|lock)|rust-toolchain(\.toml)?|config(\.toml)?)$/.test(file));
  const hash = createHash("sha256").update(JSON.stringify([root, manifest, targets,
    Object.entries(env).filter(([key]) => /^(CARGO|RUST|CC|CXX|PATH|LIB|INCLUDE)/.test(key) && key !== "RUSTC_LOG").sort()]));
  hash.update(fs.readFileSync(new URL(import.meta.url)));
  for (const file of files.sort()) {
    hash.update(file);
    try { hash.update(fs.readFileSync(path.join(root, file))); } catch { hash.update("missing"); }
  }
  const cache = path.resolve(root, env.CARGO_TARGET_DIR || "src-tauri/target", ".codeterm-nextest", hash.digest("hex"));
  const receiptPath = path.join(cache, "receipt.json");
  let receipt;
  try { receipt = JSON.parse(fs.readFileSync(receiptPath, "utf8")); } catch {}
  const stamp = file => {
    const stat = fs.statSync(file);
    return [file, stat.size, stat.mtimeMs];
  };
  let reuse = false;
  if (receipt) {
    const built = timed("cargo-freshness", () => run("cargo", ["test", "--no-run", "--locked", "--message-format=json",
      "--manifest-path", manifest, ...targets], options));
    const artifacts = built.split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line))
      .filter(row => row.reason === "compiler-artifact");
    const binaries = artifacts.filter(row => row.profile?.test && row.executable).map(row => row.executable).sort();
    try {
      reuse = artifacts.length > 0 && artifacts.every(row => row.fresh === true) && binaries.length > 0 &&
        ["binaries.json", "cargo.json"].every(file => fs.existsSync(path.join(cache, file))) &&
        JSON.stringify(binaries.map(stamp)) === JSON.stringify(receipt.binaries) &&
        receipt.manifests.every(([file, digest]) => createHash("sha256").update(fs.readFileSync(file)).digest("hex") === digest);
    } catch { reuse = false; }
  }
  if (!reuse) {
    fs.mkdirSync(cache, { recursive: true });
    fs.rmSync(receiptPath, { force: true });
    const binariesText = timed("nextest-list", () => run("cargo", ["nextest", "list", "--manifest-path", manifest,
      ...targets, "--locked", "--list-type", "binaries-only", "--message-format", "json"], options));
    const metadataText = timed("cargo-metadata", () => run("cargo", ["metadata", "--manifest-path", manifest,
      "--format-version", "1", "--locked"], options));
    const summary = JSON.parse(binariesText);
    const metadata = JSON.parse(metadataText);
    const binaries = Object.values(summary["rust-binaries"] ?? {}).map(binary => binary["binary-path"]).sort();
    const manifests = metadata.packages.map(pkg => pkg.manifest_path).sort().map(file =>
      [file, createHash("sha256").update(fs.readFileSync(file)).digest("hex")]);
    fs.writeFileSync(path.join(cache, "binaries.json"), binariesText);
    fs.writeFileSync(path.join(cache, "cargo.json"), metadataText);
    if (binaries.length) fs.writeFileSync(receiptPath, JSON.stringify({ binaries: binaries.map(stamp), manifests }));
  }
  fs.mkdirSync(output, { recursive: true });
  for (const file of ["binaries.json", "cargo.json"]) fs.copyFileSync(path.join(cache, file), path.join(output, file));
  log(`[test-loop] metadata ${reuse ? "reused after Cargo Fresh" : "refreshed"}`);
  return reuse;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [output, manifest, ...targets] = process.argv.slice(2);
    prepareNextestMetadata({ output, manifest, targets });
  } catch (error) { console.error(error.message); process.exitCode = error.status || 1; }
}
