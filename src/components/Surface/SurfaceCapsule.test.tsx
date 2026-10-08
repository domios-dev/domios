// Run: npx tsx src/components/Surface/SurfaceCapsule.test.tsx

import assert from "node:assert/strict";
import { act } from "react";
import { SurfaceCapsule } from "./SurfaceCapsule";
import { SurfaceNoticeBar } from "./SurfaceNoticeBar";
import { capsuleModel, type SurfaceState } from "./surfaceChromeModel";
import { getByRole, render, testSuite } from "./surfaceTestDom.testdata";

const suite = testSuite("SurfaceCapsule");

suite.test("the capsule is a polite status showing the model text", async () => {
  const state: SurfaceState = { owner: "Jev", activity: "clicking @e2", profile: "Andrey" };
  const { host, unmount } = await render(<SurfaceCapsule capsule={capsuleModel(state)} />);
  const status = getByRole(host, "status");
  assert.equal(status.getAttribute("aria-live"), "polite");
  assert.equal(status.textContent, capsuleModel(state).text);
  await unmount();
});

suite.test("an error notice is an alert and Dismiss fires once", async () => {
  let dismissed = 0;
  const { host, unmount } = await render(<SurfaceNoticeBar notice={{ text: "boom", tone: "error" }} onDismiss={() => { dismissed += 1; }} />);
  const alert = getByRole(host, "alert");
  assert.ok(alert.textContent?.includes("boom"), "the supplied notice text is shown");
  await act(async () => getByRole(alert, "button", "Dismiss notice").click());
  assert.equal(dismissed, 1);
  await unmount();
});

void suite.run();
