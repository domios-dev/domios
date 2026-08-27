import assert from "node:assert/strict";
import test from "node:test";
import {
  APP_ROOT_SELECTOR,
  isAppOriginUrl,
  isProbeCandidateUrl,
  noAppPageReason,
  rankAppPageCandidates,
} from "./cdp-app-page";
import { devInstanceForSlot, type DevInstance } from "./dev-instance";

const INSTANCE: DevInstance = devInstanceForSlot(0, 2);
const APP_URL = `http://127.0.0.1:${INSTANCE.vitePort}/?e2e=1`;
const NOTIF_POPUP = "about:blank";
const DEVTOOLS = "devtools://devtools/bundled/inspector.html";
const FOREIGN_APP = `http://127.0.0.1:${INSTANCE.vitePort + 7}/`;

test("the app page is matched by origin, whatever position it holds", () => {
  const first = rankAppPageCandidates([NOTIF_POPUP, DEVTOOLS, APP_URL], INSTANCE)[0];
  assert.deepEqual(first, { url: APP_URL, index: 2, criterion: "app-origin" });
});

test("position alone would have picked the wrong document", () => {
  const pages = [NOTIF_POPUP, APP_URL];
  assert.notEqual(pages[0], APP_URL, "the defect: pages()[0] is not the app page");
  assert.equal(rankAppPageCandidates(pages, INSTANCE)[0].url, APP_URL);
});

test("non-app schemes are never probed for the app root", () => {
  assert.equal(isProbeCandidateUrl(NOTIF_POPUP), false);
  assert.equal(isProbeCandidateUrl(DEVTOOLS), false);
  assert.equal(isProbeCandidateUrl(APP_URL), true);
  assert.deepEqual(
    rankAppPageCandidates([NOTIF_POPUP, DEVTOOLS], INSTANCE),
    [],
    "a WebView2 holding only stray targets offers no candidate",
  );
});

test("a sibling instance's origin is not this instance's app", () => {
  assert.equal(isAppOriginUrl(APP_URL, INSTANCE), true);
  assert.equal(isAppOriginUrl(INSTANCE.apiBase, INSTANCE), true);
  assert.equal(isAppOriginUrl(FOREIGN_APP, INSTANCE), false);
  assert.equal(isAppOriginUrl("not a url", INSTANCE), false);
});

test("an unknown origin is still probed for the app root, ranked after the origin match", () => {
  const ranked = rankAppPageCandidates([FOREIGN_APP, APP_URL], INSTANCE);
  assert.deepEqual(
    ranked.map((candidate) => [candidate.url, candidate.criterion]),
    [
      [APP_URL, "app-origin"],
      [FOREIGN_APP, "app-root"],
    ],
  );
});

test("the not-ready reason names every open target instead of guessing", () => {
  const reason = noAppPageReason([NOTIF_POPUP, DEVTOOLS]);
  assert.ok(reason.includes(APP_ROOT_SELECTOR), reason);
  assert.ok(reason.includes(NOTIF_POPUP) && reason.includes(DEVTOOLS), reason);
  assert.match(noAppPageReason([]), /\[none\]/);
});
