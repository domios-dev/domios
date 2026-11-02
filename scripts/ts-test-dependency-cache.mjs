import { createHash, randomUUID } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { createRequire, isBuiltin } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

function hashSources(hash, directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const filename = path.join(directory, entry.name);
    if (entry.isDirectory()) hashSources(hash, filename);
    else if (entry.isFile() && entry.name.endsWith(".js")) {
      hash.update(filename).update("\0").update(readFileSync(filename));
    }
  }
}

export async function prepareHappyDomBundle(root, signal) {
  const require = createRequire(path.join(root, "package.json"));
  const entryPoint = require.resolve("happy-dom");
  const { context, version } = await import("esbuild");
  const hash = createHash("sha256")
    .update(version)
    .update(readFileSync(fileURLToPath(import.meta.url)))
    .update(readFileSync(path.join(root, "pnpm-lock.yaml")))
    .update(readFileSync(path.join(path.dirname(entryPoint), "..", "package.json")));
  hashSources(hash, path.dirname(entryPoint));
  const directory = path.join(root, "node_modules", ".cache", "ts-unit");
  const filename = path.join(directory, `happy-dom-${hash.digest("hex")}.mjs`);
  if (existsSync(filename)) return filename;

  const builder = await context({
    entryPoints: [entryPoint],
    bundle: true,
    platform: "node",
    target: "node22",
    format: "esm",
    keepNames: true,
    minify: true,
    write: false,
    plugins: [{
      name: "external-packages",
      setup(builder) {
        builder.onResolve({ filter: /^[^./]|^\.[^./]|^\.\.[^/]/ }, async (args) => {
          if (args.kind === "entry-point" || args.pluginData?.externalizing) return;
          if (isBuiltin(args.path)) return { path: args.path, external: true };
          const resolved = await builder.resolve(args.path, {
            resolveDir: args.resolveDir,
            kind: args.kind,
            pluginData: { externalizing: true },
          });
          if (resolved.errors.length) return { errors: resolved.errors };
          return { path: pathToFileURL(resolved.path).href, external: true };
        });
      },
    }],
  });
  const cancel = () => void builder.cancel();
  signal?.addEventListener("abort", cancel, { once: true });
  let result;
  try {
    signal?.throwIfAborted();
    result = await builder.rebuild();
    signal?.throwIfAborted();
  } finally {
    signal?.removeEventListener("abort", cancel);
    await builder.dispose();
  }
  mkdirSync(directory, { recursive: true });
  const temporary = path.join(directory, `${randomUUID()}.tmp`);
  try {
    writeFileSync(temporary, result.outputFiles[0].contents, { flag: "wx" });
    try {
      renameSync(temporary, filename);
    } catch (error) {
      if (!existsSync(filename)) throw error;
    }
  } finally {
    rmSync(temporary, { force: true });
  }
  return filename;
}
