// Run: npx tsx src/components/Surface/SurfaceNoticeBar.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { SurfaceNoticeBar } from "./SurfaceNoticeBar";
import { getByRole, render, testSuite } from "./surfaceTestDom.testdata";

const suite = testSuite("SurfaceNoticeBar");

suite.test("an error notice is an alert and Dismiss fires once", async () => {
  let dismissed = 0;
  const { host, unmount } = await render(<SurfaceNoticeBar notice={{ text: "boom", tone: "error" }} onDismiss={() => { dismissed += 1; }} />);
  const alert = getByRole(host, "alert");
  assert.ok(alert.textContent?.includes("boom"), "the supplied notice text is shown");
  await act(async () => getByRole(host, "button", "Dismiss notice").click());
  assert.equal(dismissed, 1);
  await unmount();
});

suite.test("a warn notice is a polite status carrying its tone", async () => {
  const { host, unmount } = await render(<SurfaceNoticeBar notice={{ text: "Jev needs you: bot check", tone: "warn" }} onDismiss={() => undefined} />);
  const status = getByRole(host, "status");
  assert.equal(host.querySelector('[data-testid="surface-notice"]')?.getAttribute("data-tone"), "warn");
  assert.ok(status.textContent?.includes("Jev needs you: bot check"));
  await unmount();
});

void suite.run();
