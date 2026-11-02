import { createHash, randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

function hashTree(hash, directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    if (entry.name === 'node_modules' || entry.name === 'dist') continue;
    const filename = path.join(directory, entry.name);
    if (entry.isDirectory()) hashTree(hash, filename);
    else if (entry.isFile()) hash.update(filename).update(readFileSync(filename));
  }
}

export async function prepareTestSources(root, tests, signal, env = process.env) {
  const { context, transform, version } = await import('esbuild');
  const nodeEnvironment = env.NODE_ENV ?? 'development';
  const hash = createHash('sha256').update(version).update(JSON.stringify(tests))
    .update(nodeEnvironment)
    .update(readFileSync(fileURLToPath(import.meta.url)))
    .update(readFileSync(path.join(root, 'pnpm-lock.yaml')))
    .update(readFileSync(path.join(root, 'tsconfig.json')));
  for (const name of ['src', 'packages']) hashTree(hash, path.join(root, name));
  const parent = path.join(root, 'node_modules', '.cache', 'ts-unit');
  const indexPath = path.join(parent, `sources-${hash.digest('hex')}.json`);
  const mappings = directory => Object.fromEntries(tests.map((test, index) => [test, path.join(directory, `${index}.mjs`)]));
  if (existsSync(indexPath)) {
    const cached = JSON.parse(readFileSync(indexPath, 'utf8'));
    if (existsSync(path.join(cached.directory, 'complete'))
      && Object.entries(cached.inputs).every(([filename, expected]) => existsSync(filename)
        && createHash('sha256').update(readFileSync(filename)).digest('hex') === expected)) return mappings(cached.directory);
  }
  mkdirSync(parent, { recursive: true });
  const temporary = path.join(parent, `sources-${randomUUID()}`);
  const directory = `${temporary}-ready`;
  const temporaryIndex = `${indexPath}-${randomUUID()}.tmp`;
  const started = performance.now();
  const builder = await context({
    absWorkingDir: root,
    entryPoints: Object.fromEntries(tests.map((test, index) => [String(index), path.resolve(root, test)])),
    outdir: temporary,
    outExtension: { '.js': '.mjs' },
    bundle: true,
    splitting: false,
    platform: 'node',
    target: 'node22',
    format: 'esm',
    keepNames: true,
    minify: true,
    metafile: true,
    jsx: 'automatic',
    external: ['react', 'react/*', 'happy-dom'],
    banner: { js: 'import {createRequire as __unitRequire} from "node:module"; const require = __unitRequire(import.meta.url);' },
    plugins: [{
      name: 'source-locations-and-assets',
      setup(build) {
        build.onLoad({ filter: /\.[cm]?[jt]sx?$/ }, async args => {
          const renderer = args.path.includes(`${path.sep}react-dom${path.sep}`)
            || args.path.includes(`${path.sep}scheduler${path.sep}`);
          if (args.path.includes(`${path.sep}node_modules${path.sep}`) && !renderer) return;
          const contents = readFileSync(args.path, 'utf8');
          if (!renderer && !/import\.meta|__dirname|__filename/.test(contents)) return;
          const result = await transform(contents, {
            loader: args.path.endsWith('x') ? 'tsx' : 'ts',
            format: renderer ? 'cjs' : 'esm',
            target: 'node22',
            jsx: 'automatic',
            sourcefile: args.path,
            define: renderer ? { 'process.env.NODE_ENV': JSON.stringify(nodeEnvironment) } : {
              'import.meta.url': JSON.stringify(pathToFileURL(args.path).href),
              'import.meta.dirname': JSON.stringify(path.dirname(args.path)),
              'import.meta.filename': JSON.stringify(args.path),
              __dirname: JSON.stringify(path.dirname(args.path)),
              __filename: JSON.stringify(args.path),
            },
          });
          return { contents: result.code, loader: 'js', resolveDir: path.dirname(args.path) };
        });
        build.onLoad({ filter: /\.(png|jpe?g|gif|svg|webp|avif|ico|woff2?|ttf|mp3|wav|css)$/i }, args => ({
          contents: `export default ${args.path.endsWith('.css') ? '{}' : JSON.stringify(args.path)};`,
          loader: 'js',
        }));
      },
    }],
  });
  const cancel = () => void builder.cancel();
  signal?.addEventListener('abort', cancel, { once: true });
  try {
    signal?.throwIfAborted();
    const result = await builder.rebuild();
    signal?.throwIfAborted();
    const inputs = Object.fromEntries(Object.keys(result.metafile.inputs).map(filename => {
      const absolute = path.resolve(root, filename);
      return [absolute, createHash('sha256').update(readFileSync(absolute)).digest('hex')];
    }));
    writeFileSync(path.join(temporary, 'inputs.json'), JSON.stringify(inputs));
    writeFileSync(path.join(temporary, 'complete'), '');
    renameSync(temporary, directory);
    writeFileSync(temporaryIndex, JSON.stringify({ directory, inputs }));
    renameSync(temporaryIndex, indexPath);
    process.stdout.write(`TS unit compilation: ${tests.length} entries, ${Math.round(performance.now() - started)}ms (outside execution budget)\n`);
  } finally {
    signal?.removeEventListener('abort', cancel);
    await builder.dispose();
    rmSync(temporary, { recursive: true, force: true });
    rmSync(temporaryIndex, { force: true });
  }
  return mappings(directory);
}
