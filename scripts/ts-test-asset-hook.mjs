// Caller: CI frontend-release.yml; scripts ci-scope.mjs, run-ts-unit-tests.mjs, tests/ci-scope.test.mjs; skill gates
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

const happyDom = new URL(import.meta.url).searchParams.get("happy-dom");
register("./ts-test-assets.mjs", pathToFileURL(import.meta.filename), {
  data: { happyDom },
});

if (happyDom) {
  const resolveFilename = Module._resolveFilename;
  Module._resolveFilename = function (specifier, ...args) {
    return resolveFilename.call(this, specifier === "happy-dom" ? happyDom : specifier, ...args);
  };
}

// tsx transpiles TSX to CommonJS, so an asset import arrives through require()
// and never reaches the ESM hooks above. Both paths resolve to the same string.
for (const extension of ASSET_EXTENSIONS) {
  Module._extensions[extension] = (module, filename) => {
    module.exports = filename;
  };
}

Module._extensions[".css"] = (module) => {
  module.exports = {};
};
