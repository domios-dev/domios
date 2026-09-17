// Vite resolves an asset import to a URL string. Node does not, so a component
// that imports one is untestable under the unit runner unless we do the same.
const ASSET = /\.(png|jpe?g|gif|svg|webp|avif|ico|woff2?|ttf|mp3|wav)$/i;

export async function resolve(specifier, context, next) {
  if (!ASSET.test(specifier)) return next(specifier, context);
  const url = context.parentURL
    ? new URL(specifier, context.parentURL).href
    : specifier;
  return { url, format: "module", shortCircuit: true };
}

export async function load(url, context, next) {
  if (!ASSET.test(new URL(url).pathname)) return next(url, context);
  return {
    format: "module",
    shortCircuit: true,
    source: `export default ${JSON.stringify(url)};`,
  };
}
