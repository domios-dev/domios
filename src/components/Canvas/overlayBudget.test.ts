// Run: npx tsx src/components/Canvas/overlayBudget.test.ts

import { isRichOverlayNodeKind, shouldSuppressWorldRichContent } from "./overlayBudget";
import { __resetOverlayBudgetForTest } from "./overlayBudgetFlag";

let passed = 0;
let failed = 0;

function ok(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

console.log("overlay budget helpers:");
{
  __resetOverlayBudgetForTest();
  ok("plan/task/spec are rich overlay kinds", isRichOverlayNodeKind("plan") && isRichOverlayNodeKind("task") && isRichOverlayNodeKind("spec"));
  ok("milestone is not a rich overlay kind", !isRichOverlayNodeKind("milestone"));
  ok("budget off does not suppress world rich content", !shouldSuppressWorldRichContent("task"));

  const store = new Map<string, string>();
  (globalThis as { window?: Window }).window = {
    localStorage: {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => { store.set(k, v); },
      removeItem: (k: string) => { store.delete(k); },
    },
  } as Window;
  store.set("canvas.overlayBudget", "true");
  __resetOverlayBudgetForTest();
  ok("budget on suppresses task rich content in world layer", shouldSuppressWorldRichContent("task"));
  ok("budget on suppresses plan rich content in world layer", shouldSuppressWorldRichContent("plan"));
  ok("budget on leaves milestone untouched", !shouldSuppressWorldRichContent("milestone"));
  store.delete("canvas.overlayBudget");
  __resetOverlayBudgetForTest();
}

if (failed) {
  console.error(`\n${failed} failed, ${passed} passed`);
  process.exit(1);
}
console.log(`\n${passed} passed`);
