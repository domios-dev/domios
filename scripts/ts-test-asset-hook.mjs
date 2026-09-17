import Module from "node:module";
import { register } from "node:module";
import { pathToFileURL } from "node:url";

const ASSET_EXTENSIONS = [
  ".png",
  ".jpg",
  ".jpeg",
  ".gif",
  ".svg",
  ".webp",
  ".avif",
  ".ico",
  ".woff",
  ".woff2",
  ".ttf",
  ".mp3",
  ".wav",
];

register("./ts-test-assets.mjs", pathToFileURL(import.meta.filename));

// tsx transpiles TSX to CommonJS, so an asset import arrives through require()
// and never reaches the ESM hooks above. Both paths resolve to the same string.
for (const extension of ASSET_EXTENSIONS) {
  Module._extensions[extension] = (module, filename) => {
    module.exports = filename;
  };
}
