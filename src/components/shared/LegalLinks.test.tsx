import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
const {openUrl} = vi.hoisted(() => ({openUrl:vi.fn()}));
vi.mock("../../lib/nativeBridge", () => ({nativeBridge:{openUrl}}));
import { LegalLink, SubscriptionTermsNotice, legalMenuItems } from "./LegalLinks";
import { siteLinks } from "../../../shared/siteLinks";
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => {openUrl.mockClear();});

async function clickLinks(element: React.ReactElement) {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(element));
    const links = Array.from(host.querySelectorAll("a"));
    for (const link of links) await act(async () => link.click());
    return links.map(link => ({name:link.textContent,href:link.href}));
  } finally {await act(async () => root.unmount());host.remove();}
}

it("opens each legal document in the system browser", async () => {
  const links = await clickLinks(<><LegalLink document="terms" /><LegalLink document="privacy" /><LegalLink document="license" /></>);
  expect(links).toEqual([
    {name:"Terms of Service",href:siteLinks.terms},{name:"Privacy Policy",href:siteLinks.privacy},{name:"License",href:siteLinks.license},
  ]);
  expect(openUrl.mock.calls).toEqual([[siteLinks.terms],[siteLinks.privacy],[siteLinks.license]]);
});

it("links the terms and privacy policy from the subscription notice", async () => {
  const links = await clickLinks(<SubscriptionTermsNotice />);
  expect(links).toEqual([{name:"Terms",href:siteLinks.terms},{name:"Privacy",href:siteLinks.privacy}]);
  expect(openUrl.mock.calls).toEqual([[siteLinks.terms],[siteLinks.privacy]]);
});

it("builds menu items that open the requested documents", () => {
  const items = legalMenuItems(["terms","privacy"]);
  expect(items.map(item => item.label)).toEqual(["Terms of Service","Privacy Policy"]);
  for (const item of items) item.onClick?.();
  expect(openUrl.mock.calls).toEqual([[siteLinks.terms],[siteLinks.privacy]]);
});
