/**
 * WHAT: Canvas diagnostic capture opens selected plans and writes board screenshots.
 * WHY: Visual debugging needs a quick operator-only probe outside assertion-gated acceptance specs.
 * ORACLE: Each target plan reaches the canvas capture path and writes the requested artifact.
 */
import { test } from "../../helpers/fixtures";
import { openCanvas, captureBoard } from "../../helpers/canvas-inspect";
import fs from "node:fs";

const PLANS = [
  { id: "pl_c8a72a9a870743ab81fb8ac94e0e459f", label: "dogfood (Spec25 ⊥ Plan6)" },
  { id: "pl_2e932e82ad614cdd9f08072480b0857e", label: "substrate (Spec4 ⊥ Plan2)" },
  { id: "pl_0d48ee2925404f4db5b7664b566ccaa7", label: "usage (spec-LESS → no spec box)" },
  { id: "pl_a022a22d8f4e42d983cca288bd264e43", label: "ergonomics (Spec7 ⊥ Plan1)" },
];

test("@diag audit boxes", async ({ ui }) => {
  test.setTimeout(90_000);
  const { page } = ui;
  await openCanvas(page);
  const results: any[] = [];
  for (const p of PLANS) {
    await page.evaluate(async (id) => {
      const token = localStorage.getItem(`codeterm-web-token:${window.location.origin}`) || "";
      await fetch("/api/canvas/focus", { method: "POST", headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` }, body: JSON.stringify({ plan_id: id }) });
    }, p.id);
    // PW_JUSTIFIED_SLEEP: fixed 900ms sampling/settle window; no app event exists for this probe.
    await page.waitForTimeout(900); // diag-only spec: fixed observation window
    const r = await page.evaluate((id) => {
      const spec = document.querySelector(`[data-testid="canvas-spec-node"][data-plan-id="${id}"]`);
      const plan = document.querySelector(`[data-testid="canvas-plan-node"][data-plan-id="${id}"]`);
      const rows = (n: Element | null) => (n ? n.querySelectorAll('[data-testid="canvas-spec-block"], [data-testid="canvas-plan-row"], [data-section-id]').length : -1);
      const goal = plan?.querySelector('[data-testid="canvas-plan-goal"]');
      return { specNode: !!spec, specRows: rows(spec), planRows: rows(plan), hasGoal: !!goal };
    }, p.id);
    results.push({ ...p, ...r });
  }
  fs.writeFileSync("/tmp/diag-dump.json", JSON.stringify(results, null, 2));
  await captureBoard(page, "diag-audit");
});
