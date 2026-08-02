// Run: npx tsx src/components/shared/ActivityDots.test.tsx

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { ActivityDots } from "./ActivityDots";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({
  window,
  document: window.document,
  HTMLElement: window.HTMLElement,
  Element: window.Element,
})) Object.defineProperty(globalThis, name, { value, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);

async function run() {
  await act(async () => root.render(
    <ActivityDots size="small" tone="thinking" rhythm="streaming" data-testid="dots" />,
  ));
  const dots = host.querySelector('[data-testid="dots"]');
  assert(dots, "ActivityDots renders");
  assert.equal(dots.getAttribute("aria-hidden"), "true");
  assert.equal(dots.children.length, 3, "small dots render three dots");
  assert(dots.classList.contains("activity-dots--small"));
  assert(dots.classList.contains("activity-dots--thinking"));
  assert(dots.classList.contains("activity-dots--streaming"));

  await act(async () => root.render(<ActivityDots idle />));
  assert(host.querySelector(".activity-dots--idle"), "idle state is static");

  await act(async () => root.render(<ActivityDots size="micro" />));
  assert.equal(host.querySelector(".activity-dots--micro")?.children.length, 1, "micro renders one breathing dot");

  await act(async () => root.render(<ActivityDots size="micro" variant="workspace-badge" />));
  const workspaceBadgeDots = host.querySelector(".activity-dots--workspace-badge");
  assert(workspaceBadgeDots, "workspace badge variant renders");
  assert.equal(workspaceBadgeDots.children.length, 2, "workspace badge variant renders exactly two dots");

  const css = readFileSync(new URL("../../styles/agent-state.css", import.meta.url), "utf8");
  const microRule = css.match(/\.activity-dots--micro\s*\{([^}]*)\}/s);
  assert(microRule, "micro geometry rule exists");
  assert.match(microRule[1], /--activity-dot-size:\s*2\.5px;/);
  assert.match(microRule[1], /--activity-dot-gap:\s*1\.25px;/);
  assert.match(microRule[1], /--activity-dot-jump:\s*1\.25px;/);
  assert.match(css, /\.activity-dots--small\s*\{[^}]*--activity-dot-size:\s*3px;[^}]*--activity-dot-gap:\s*2px;/s);
  assert.match(css, /\.activity-dots--medium\s*\{[^}]*--activity-dot-size:\s*5px;[^}]*--activity-dot-gap:\s*3px;/s);
  const reducedMotion = css.slice(css.indexOf("@media (prefers-reduced-motion: reduce)"));
  assert.match(
    reducedMotion,
    /\.activity-dots i\s*\{[^}]*animation:\s*none !important;[^}]*opacity:\s*0\.85;[^}]*transform:\s*scale\(1\);/s,
    "reduced motion keeps the static dot cue",
  );
  assert.doesNotMatch(
    css.match(/\.activity-dots--(?:accent|thinking|muted|on-accent)\s*\{[^}]*\}/gs)?.join("\n") ?? "",
    /#[\da-f]{3,8}\b|\brgb\(|\b(?:white|black)\b/i,
    "ActivityDots tones derive exclusively from theme tokens",
  );
  const phaseDots = css.match(/\.ct-phase-dots\s*\{([^}]*)\}/s);
  assert(phaseDots, "legacy phase dots rule exists");
  assert(!/--activity-dot-[\w-]+\s*:/.test(phaseDots[1]), "legacy phase dots do not own geometry vars");
  for (const size of ["micro", "small", "medium"]) {
    const rule = css.match(new RegExp(`\\.activity-dots--${size}\\s*\\{([^}]*)\\}`, "s"));
    assert(rule, `${size} geometry rule exists`);
    for (const variable of ["size", "gap", "jump"]) {
      assert.match(rule[1], new RegExp(`--activity-dot-${variable}:`), `${size} owns ${variable}`);
    }
  }
  assert.equal((css.match(/@keyframes ct-dots-squash-jump/g) ?? []).length, 1, "one shared dot keyframe family");
  assert.match(css, /animation:\s*ct-dots-squash-jump[^;]*var\(--ct-ease-squash-jump\)/);
  for (const stop of ["0%, 70%, 100%", "10%", "24%", "42%", "54%"]) {
    assert(css.includes(stop), `richer shipped squash-jump stop ${stop} is retained`);
  }
  assert.doesNotMatch(phaseDots[1], /--activity-dot-(?:size|gap|jump):/, "ct-phase-dots cannot win geometry by source order");

  const globalsCss = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");
  const workspaceCardSource = readFileSync(new URL("../Sidebar/WorkspaceCardGrid.tsx", import.meta.url), "utf8");
  assert.match(
    workspaceCardSource,
    /className="sidebar-workspace-card-working"[\s\S]*?<ActivityDots size="micro" tone="on-accent" variant="workspace-badge" \/>/,
    "workspace-card badge selects the shared two-dot on-accent variant",
  );
  assert.match(css, /\.activity-dots--on-accent\s*\{\s*color:\s*var\(--ct-on-accent\);\s*\}/);
  assert.match(css, /\.activity-dots--thinking\s*\{\s*color:\s*color-mix\(in srgb, var\(--ct-magenta\) 72%, var\(--ct-fg\)\);\s*\}/);
  assert.doesNotMatch(workspaceCardSource, /tone="muted"/, "filled workspace bubble never mixes toward page foreground");
  assert.doesNotMatch(
    globalsCss,
    /\.sidebar-workspace-card-working\s+\.activity-dots\s*\{[^}]*--activity-dot-(?:size|gap|jump)\s*:/s,
    "workspace-card badge must not re-tune shared micro geometry",
  );
  const workspaceCardRule = globalsCss.match(/\.sidebar-workspace-card\s*\{([^}]*)\}/s);
  const workspaceSelectRule = globalsCss.match(/\.sidebar-workspace-card-select\s*\{([^}]*)\}/s);
  const workspaceInitialsRule = globalsCss.match(/\.sidebar-workspace-card-initials\s*\{([^}]*)\}/s);
  const workspaceBadgeRule = globalsCss.match(/\.sidebar-workspace-card-working\s*\{([^}]*)\}/s);
  assert(workspaceCardRule && workspaceSelectRule && workspaceInitialsRule && workspaceBadgeRule);
  assert.match(workspaceSelectRule[1], /align-items:\s*center;[^}]*justify-content:\s*center;/s);
  const px = (rule: string, property: string) => {
    const match = rule.match(new RegExp(`${property}:\\s*([\\d.]+)px;`));
    assert(match, `${property} pixel value exists`);
    return Number(match[1]);
  };
  const cardHeight = px(workspaceCardRule[1], "height");
  const initialsHeight = px(workspaceInitialsRule[1], "font-size");
  const badgeTop = px(workspaceBadgeRule[1], "top");
  const badgeWidth = px(workspaceBadgeRule[1], "width");
  const badgeHeight = px(workspaceBadgeRule[1], "height");
  const microSize = px(microRule[1], "--activity-dot-size");
  const microJump = px(microRule[1], "--activity-dot-jump");
  assert((microSize * 2) + 1.25 <= badgeWidth, "two shared micro dots and their gap fit the workspace badge width");
  assert(microSize + microJump <= badgeHeight, "shared micro jump fits the workspace badge height");
  assert.match(workspaceBadgeRule[1], /box-sizing:\s*border-box;[^}]*padding-bottom:\s*1px;/s, "pill reserves room for the landing squash");
  assert(badgeTop + badgeHeight < (cardHeight - initialsHeight) / 2, "workspace badge stays above centered initials");
  const cubeRules = [".sidebar-workspace-card {", ".sidebar-workspace-card[data-selected=\"true\"]", ".sidebar-workspace-card-select:hover"];
  for (const rule of cubeRules) assert(globalsCss.includes(rule), `cube-grid surface ${rule} remains owned by globals`);
  for (const keyframe of [
    "sidebar-hierarchy-breathe",
    "sidebar-hierarchy-drift",
    "tab-focus-pulse",
    "breathing-bar",
    "agent-icon-working-spin",
    "ct-spin",
  ]) {
    assert.equal(globalsCss.includes(keyframe), false, `${keyframe} is fully removed from globals.css`);
  }

  await act(async () => root.unmount());
  console.log("activity dots tests: passed");
}

void run();
