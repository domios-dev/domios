import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import { DeferredLoader } from "./DeferredLoader";

const present = renderToStaticMarkup(
  <DeferredLoader state="present" className="loader-shape" aria-label="Loading" />,
);
assert.match(present, /class="ct-deferred-loader loader-shape"/);
assert.match(present, /data-transition-state="present"/);
assert.match(present, /aria-label="Loading"/);

const exiting = renderToStaticMarkup(<DeferredLoader state="exiting" />);
assert.match(exiting, /data-transition-state="exiting"/);

console.log("DeferredLoader tests: passed");
