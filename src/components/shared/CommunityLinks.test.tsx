import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
const {openUrl} = vi.hoisted(() => ({openUrl:vi.fn()}));
vi.mock("../../lib/nativeBridge", () => ({nativeBridge:{openUrl}}));
import { CommunityLinks } from "./CommunityLinks";
import { siteLinks } from "../../../shared/siteLinks";
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT = true;
it("opens app community destinations in the system browser", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<CommunityLinks />));
    const links = Array.from(host.querySelectorAll("a"));
    expect(links.map(link => ({name:link.getAttribute("aria-label") ?? link.textContent,href:link.href}))).toEqual([
      {name:"Telegram",href:siteLinks.telegram},{name:"Discord",href:siteLinks.discord},
    ]);
    for (const link of links) await act(async () => link.click());
    expect(openUrl.mock.calls).toEqual([[siteLinks.telegram],[siteLinks.discord]]);
  } finally {await act(async () => root.unmount());host.remove();}
});
