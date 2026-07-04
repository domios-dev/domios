// Run: npx tsx src/components/Canvas/planView/PlanSwitcher.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  PlanSwitcher,
  type PlanKebabAction,
  type PlanSwitcherOption,
} from "./PlanSwitcher";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const window = new Window({ url: "http://localhost/plan-switcher" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });

// Stub confirm so archive tests work without real dialogs.
(window as unknown as { confirm: (msg: string) => boolean }).confirm = () => true;

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

function opts(n: number, overrides?: Partial<PlanSwitcherOption>): PlanSwitcherOption[] {
  return Array.from({ length: n }, (_, i) => ({ id: `p-${i}`, title: `Plan ${i}`, ...overrides }));
}

test("renders nothing for a single plan", async () => {
  const { host } = await render(<PlanSwitcher plans={opts(1)} selectedId="p-0" onSelect={() => {}} />);
  assert(!host.querySelector('[data-testid="plan-switcher"]'), "no switcher");
});

test("segmented buttons for <=3 active plans; click selects", async () => {
  let picked = "";
  const { host } = await render(
    <PlanSwitcher plans={opts(3)} selectedId="p-0" onSelect={(id) => (picked = id)} />,
  );
  const options = host.querySelectorAll('[data-testid="plan-switcher-option"]');
  assert(options.length === 3, `segmented count=${options.length}`);
  assert(!host.querySelector('[data-testid="plan-switcher-select"]'), "no native dropdown");
  const active = host.querySelector('[data-testid="plan-switcher-option"][data-plan-id="p-0"]');
  assert(active?.getAttribute("data-active") === "true", "selected marked active");
  const target = host.querySelector(
    '[data-testid="plan-switcher-option"][data-plan-id="p-2"]',
  ) as HTMLElement;
  await act(async () => {
    target.click();
  });
  assert(picked === "p-2", `selected=${picked}`);
});

test("overflow mode for >3 active plans; combo trigger and listbox appear", async () => {
  let picked = "";
  const { host } = await render(
    <PlanSwitcher plans={opts(5)} selectedId="p-1" onSelect={(id) => (picked = id)} />,
  );
  const trigger = host.querySelector('[data-testid="plan-switcher-combo-trigger"]') as HTMLElement;
  assert(trigger, "combo trigger present");
  assert(!host.querySelector('[data-testid="plan-switcher-select"]'), "no native select");
  assert(!host.querySelector('[data-testid="plan-switcher-option"]'), "no segmented buttons");
  assert(!host.querySelector('[data-testid="plan-switcher-listbox"]'), "listbox closed initially");

  await act(async () => {
    trigger.click();
  });
  const listbox = host.querySelector('[data-testid="plan-switcher-listbox"]');
  assert(listbox, "listbox opened on trigger click");
  assert(listbox?.getAttribute("role") === "listbox", "listbox role");

  const optBtns = listbox?.querySelectorAll(".plan-switcher-opt-select");
  assert(optBtns?.length === 5, `5 options, got ${optBtns?.length}`);

  const p4Btn = Array.from(optBtns ?? []).find(
    (el) => el.closest("[data-plan-id='p-4']"),
  ) as HTMLElement;
  await act(async () => {
    p4Btn.click();
  });
  assert(picked === "p-4", `picked=${picked}`);
  assert(!host.querySelector('[data-testid="plan-switcher-listbox"]'), "listbox closes on select");
});

test("kebab click does not change selection (stopPropagation)", async () => {
  let picked = "";
  const { host } = await render(
    <PlanSwitcher plans={opts(2)} selectedId="p-0" onSelect={(id) => (picked = id)} />,
  );
  const kebab = host.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="p-1"]',
  ) as HTMLElement;
  assert(kebab, "kebab present on non-selected plan");
  await act(async () => {
    kebab.click();
  });
  assert(picked === "", `selection unchanged, got picked=${picked}`);
});

test("kebab opens menu with correct aria attributes", async () => {
  const { host } = await render(
    <PlanSwitcher plans={opts(2)} selectedId="p-0" onSelect={() => {}} />,
  );
  const kebab = host.querySelector('[data-testid="plan-switcher-kebab"]') as HTMLElement;
  assert(kebab?.getAttribute("aria-haspopup") === "menu", "aria-haspopup=menu");
  assert(kebab?.getAttribute("aria-expanded") === "false", "aria-expanded=false initially");
  await act(async () => {
    kebab.click();
  });
  assert(kebab.getAttribute("aria-expanded") === "true", "aria-expanded=true when open");
  assert(host.querySelector('[data-testid="plan-switcher-menu"]'), "menu visible");
  assert(
    host.querySelector('[data-testid="plan-switcher-menu"]')?.getAttribute("role") === "menu",
    "role=menu",
  );
});

test("kebab menu items: Rename, Copy plan ID, Archive", async () => {
  const { host } = await render(
    <PlanSwitcher plans={opts(2)} selectedId="p-0" onSelect={() => {}} />,
  );
  const kebab = host.querySelector('[data-testid="plan-switcher-kebab"]') as HTMLElement;
  await act(async () => {
    kebab.click();
  });
  const items = host.querySelectorAll('[role="menuitem"]');
  const labels = Array.from(items).map((el) => el.textContent?.trim());
  assert(labels.includes("Rename"), "Rename present");
  assert(labels.includes("Copy plan ID"), "Copy plan ID present");
  assert(labels.some((l) => l?.startsWith("Archive")), "Archive present");
});

test("rename fires onAction with kind=rename", async () => {
  const actions: PlanKebabAction[] = [];
  const { host } = await render(
    <PlanSwitcher
      plans={opts(2)}
      selectedId="p-0"
      onSelect={() => {}}
      onAction={(a) => actions.push(a)}
    />,
  );
  const kebab = host.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="p-0"]',
  ) as HTMLElement;
  await act(async () => {
    kebab.click();
  });
  const rename = Array.from(host.querySelectorAll('[role="menuitem"]')).find(
    (el) => el.textContent?.trim() === "Rename",
  ) as HTMLElement;
  await act(async () => {
    rename.click();
  });
  const action = actions[0];
  assert(action?.kind === "rename", `action=${JSON.stringify(action)}`);
  assert(
    action.planId === "p-0",
    "planId=p-0",
  );
  assert(!host.querySelector('[data-testid="plan-switcher-menu"]'), "menu closes after action");
});

test("archive fires onAction and auto-selects next plan when archiving selected", async () => {
  let picked = "";
  const actions: PlanKebabAction[] = [];
  const { host } = await render(
    <PlanSwitcher
      plans={opts(3)}
      selectedId="p-0"
      onSelect={(id) => (picked = id)}
      onAction={(a) => actions.push(a)}
    />,
  );
  const kebab = host.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="p-0"]',
  ) as HTMLElement;
  await act(async () => {
    kebab.click();
  });
  const archiveBtn = Array.from(host.querySelectorAll('[role="menuitem"]')).find(
    (el) => el.textContent?.includes("Archive"),
  ) as HTMLElement;
  await act(async () => {
    archiveBtn.click();
  });
  const action = actions[0];
  assert(action?.kind === "archive", `action.kind=${action?.kind}`);
  assert(
    action.planId === "p-0",
    "planId=p-0",
  );
  assert(picked !== "" && picked !== "p-0", `selection fallback applied, picked=${picked}`);
});

test("Open plan document appears only when docUrl is provided", async () => {
  const plansWithDoc: PlanSwitcherOption[] = [
    { id: "p-0", title: "Plan 0", docUrl: "https://example.com/doc" },
    { id: "p-1", title: "Plan 1" },
  ];
  const { host } = await render(
    <PlanSwitcher plans={plansWithDoc} selectedId="p-0" onSelect={() => {}} />,
  );
  const kebabWithDoc = host.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="p-0"]',
  ) as HTMLElement;
  await act(async () => {
    kebabWithDoc.click();
  });
  const items0 = Array.from(host.querySelectorAll('[role="menuitem"]')).map((el) =>
    el.textContent?.trim(),
  );
  assert(items0.includes("Open plan document"), "doc link present when docUrl set");
  await act(async () => {
    kebabWithDoc.click();
  }); // close

  const kebabNoDoc = host.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="p-1"]',
  ) as HTMLElement;
  await act(async () => {
    kebabNoDoc.click();
  });
  const items1 = Array.from(host.querySelectorAll('[role="menuitem"]')).map((el) =>
    el.textContent?.trim(),
  );
  assert(!items1.includes("Open plan document"), "doc link absent when docUrl missing");
});

test("overflow mode: kebab inside listbox fires action without closing listbox", async () => {
  const actions: PlanKebabAction[] = [];
  const { host } = await render(
    <PlanSwitcher
      plans={opts(5)}
      selectedId="p-0"
      onSelect={() => {}}
      onAction={(a) => actions.push(a)}
    />,
  );
  const trigger = host.querySelector('[data-testid="plan-switcher-combo-trigger"]') as HTMLElement;
  await act(async () => {
    trigger.click();
  });
  const listbox = host.querySelector('[data-testid="plan-switcher-listbox"]');
  assert(listbox, "listbox open");

  const kebab = listbox?.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="p-2"]',
  ) as HTMLElement;
  assert(kebab, "kebab inside listbox");
  await act(async () => {
    kebab.click();
  });
  assert(host.querySelector('[data-testid="plan-switcher-menu"]'), "menu open");
  assert(host.querySelector('[data-testid="plan-switcher-listbox"]'), "listbox still open");

  const rename = Array.from(host.querySelectorAll('[role="menuitem"]')).find(
    (el) => el.textContent?.trim() === "Rename",
  ) as HTMLElement;
  await act(async () => {
    rename.click();
  });
  const action = actions[0];
  assert(action?.kind === "rename", `action.kind=${action?.kind}`);
});

test("archived plans show toggle in segmented mode; Unarchive fires onAction", async () => {
  const actions: PlanKebabAction[] = [];
  const plans: PlanSwitcherOption[] = [
    { id: "p-active", title: "Active Plan" },
    { id: "p-2", title: "Active Plan 2" },
    { id: "p-archived", title: "Old Plan", archived: true },
  ];
  const { host } = await render(
    <PlanSwitcher
      plans={plans}
      selectedId="p-active"
      onSelect={() => {}}
      onAction={(a) => actions.push(a)}
    />,
  );
  assert(!host.querySelector('[data-testid="plan-switcher-archived-list"]'), "footer hidden initially");
  const toggle = host.querySelector('[data-testid="plan-switcher-archived-toggle"]') as HTMLElement;
  assert(toggle, "archived toggle button present");
  assert(toggle.textContent?.includes("1 archived"), "count shown");
  await act(async () => {
    toggle.click();
  });
  const archivedList = host.querySelector('[data-testid="plan-switcher-archived-list"]');
  assert(archivedList, "archived list visible after toggle");
  const archivedKebab = archivedList?.querySelector(
    '[data-testid="plan-switcher-kebab"][data-plan-id="p-archived"]',
  ) as HTMLElement;
  assert(archivedKebab, "kebab on archived plan");
  await act(async () => {
    archivedKebab.click();
  });
  const unarchive = Array.from(host.querySelectorAll('[role="menuitem"]')).find(
    (el) => el.textContent?.trim() === "Unarchive",
  ) as HTMLElement;
  assert(unarchive, "Unarchive action in archived plan menu");
  await act(async () => {
    unarchive.click();
  });
  const action = actions[0];
  assert(action?.kind === "unarchive", `action.kind=${action?.kind}`);
  assert(
    action.planId === "p-archived",
    "planId=p-archived",
  );
});

test("archived plans appear non-selectable in overflow listbox footer with Unarchive", async () => {
  const actions: PlanKebabAction[] = [];
  const plans: PlanSwitcherOption[] = [
    { id: "p-0", title: "Active A" },
    { id: "p-1", title: "Active B" },
    { id: "p-2", title: "Active C" },
    { id: "p-3", title: "Active D" },
    { id: "p-archived", title: "Old Plan", archived: true },
  ];
  const { host } = await render(
    <PlanSwitcher
      plans={plans}
      selectedId="p-0"
      onSelect={() => {}}
      onAction={(a) => actions.push(a)}
    />,
  );
  const trigger = host.querySelector('[data-testid="plan-switcher-combo-trigger"]') as HTMLElement;
  await act(async () => {
    trigger.click();
  });
  const listbox = host.querySelector('[data-testid="plan-switcher-listbox"]') as HTMLElement;
  const archivedRow = listbox.querySelector('[data-plan-id="p-archived"]');
  assert(archivedRow, "archived plan in listbox");
  assert(archivedRow.classList.contains("plan-switcher-opt-archived"), "muted class");
  assert(archivedRow.getAttribute("aria-disabled") === "true", "aria-disabled");
  const selectBtn = archivedRow.querySelector(".plan-switcher-opt-select") as HTMLButtonElement;
  assert(selectBtn.disabled, "select button disabled");

  const kebab = archivedRow.querySelector('[data-testid="plan-switcher-kebab"]') as HTMLElement;
  await act(async () => {
    kebab.click();
  });
  const unarchive = Array.from(host.querySelectorAll('[role="menuitem"]')).find(
    (el) => el.textContent?.trim() === "Unarchive",
  ) as HTMLElement;
  assert(unarchive, "Unarchive action in archived plan menu");
  await act(async () => {
    unarchive.click();
  });
  const action = actions[0];
  assert(action?.kind === "unarchive", `action.kind=${action?.kind}`);
  assert(
    action.planId === "p-archived",
    "planId=p-archived",
  );
});

let passed = 0;
(async () => {
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(e);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} PlanSwitcher tests passed`);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
