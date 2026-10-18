# Built-in Browser journey

Purpose: prove public navigation/form use and honest bounded behavior at difficult page boundaries. Core: real form, state flags, stale refs, event waits, clean-profile restrictions and viewer selection.

## Preconditions / fixtures

Shared DEV setup, an explicitly authorized owned agent as Browser caller, and no foreign Browser use. `dc --as-tab "$OWNER" plugin browser status` then `open` if absent. Default is a temporary clean session; an existing persistent profile may be selected only within the owner's grant. Discover current `docs BROWSER`; refs have a capture prefix and are opaque.

## Real-page steps

1. `dc --as-tab "$OWNER" plugin browser open https://httpbin.org/forms/post`; `snapshot`. Type textbox `Customer name:` with a synthetic name, snapshot; click radio `Medium`, snapshot; click checkbox `Extra Cheese`, snapshot. Require typed value, [focused] and correct checked values.
2. `goto https://www.selenium.dev/selenium/web/web-form.html`; snapshot. Require Disabled input [disabled], Readonly input [readonly], and options [selected]. Click combobox `Dropdown (select)`, press ArrowDown, press Enter, snapshot: One selected. Type Text input, snapshot; click Submit, then explicitly `wait --text "Form submitted"`; snapshot and text must show Received and the submitted value/select in the URL. No personal information in these public test forms.
3. `profiles`; save names only. `open --profile <unique-nonexistent-name>` must refuse with nonzero exit and leave the existing profile list unchanged. Agents list/select existing profiles, but cannot create/delete persistent profiles; user-only management is not permission for an agent to drive it through UI. Do not select an owner's signed-in profile just to fill coverage.

## Controlled boundary steps

Start `tools/browser-fixture-server.mjs` in an owned DEV utility server shell, no compile. It binds loopback, writes its real port/readiness file and waits on explicit gate releases. Preserve server/tab/PID; servers have no idle timeout. Visit its root and snapshot: Required name [required], Disabled field [disabled], Readonly field [readonly]. Native validation after Submit should focus/mark the empty required input [invalid].

- Capture a ref, take a NEW snapshot, and try the old ref: refuse. Then click SPA navigate; try a pre-navigation ref: refuse again. Re-snapshot requires `/spa` and SPA destination. Stale refs must never hit a different field.
- Use `tools/held-navigation-check.mjs`: click Held navigation once while `/held` has no response. Record whether the click finishes before `/release`. Under a contract promising action-settle, early success is FAIL. Then release via the fixture endpoint, `wait --text "Held destination complete"`, snapshot the destination. `wait --text JOY_NEVER_RENDERED` must fail its deadline (nonzero). No timer makes a page ready.
- Iframe: root renders Frame name and Frame action. Snapshot then type textbox Frame name; require frame field in the frame's own DOM, not a fabricated main-document field. If snapshot excludes it and refuses, record the boundary. Do not switch to standalone automation to claim plugin PASS.
- Popup: click Open own popup once. Record owned Browser target/opener IDs and URL; then inspect what `snapshot`/`status` actually track. A created popup outside the tracked target is not completed popup interaction. Close only fixture-owned targets through Browser cleanup.

## Text selection through the viewer

Use the actual DEV window containing the owned Browser surface. Take control, then drag the known fixture paragraph through the stream canvas. Record frame/device/viewport metadata and pointer coordinates. Wait for the remote page's selectionchange event and read `getSelection().toString()` over the SAME Browser CDP connection. Require the intended substring and a visual selection screenshot. Agent refs must invalidate after viewer input. Copy/paste needs separate clipboard permission; do not inspect/overwrite an unrelated clipboard. Direct DOM Range selection or direct remote CDP input does not prove the viewer's input bridge.

Joy's native CDP attempt selected/expanded Choice instead of the paragraph; selection remained empty. Input mapping versus helper geometry is unresolved, so viewer selection NOT VERIFIED. The failed helper is deliberately not a default runner; see tools/history.md. Do not extend deadlines or repeat it blindly.

## Current results / localization

On a73199a03: real forms/state flags/default clean/profile refusal PASS; SPA stale refs PASS; explicit existing-text wait and missing-text deadline PASS. Held navigation click returned success in42ms while response was withheld: FAIL against automatic navigation/network-settle expectation. Source `browser/verbs.rs::page_verb` click returns after click_node and reads the old shared URL; `goto`/history ignore their event-wait timeout result. Explicit DOM wait uses a MutationObserver and a bounded failure timer. These are source facts; no fix was executed.

Iframe FAIL/limitation: screenshot shows field, same-origin contentDocument has input, AX snapshot only has Iframe QA frame; type refuses. Localization: `verbs.rs::snapshot` calls Accessibility.getFullAXTree on tracked PageChannel without fetching child-frame trees. Popup creation PASS, popup interaction NOT AVAILABLE through current tracked-page verbs: target/opener exist, snapshot stays on parent. Separate product acceptance scope from a new capability request.

Cleanup: copy evidence before closing Browser (owner pad is ephemeral), close Browser and owned server utility, require process exit and removed workspace. Never delete named profiles. Generalized tools are syntax-checked drafts, not an executed future release run.

Automation: snapshot/state/ref mechanisms and deadline decisions in focused unit tests; one real-page wiring journey and an event-gated navigation fixture. Keep network-settle contract explicit rather than declaring elapsed idle successful.

Follow-up observed during prod account QA: URL query parameter `code_challenge` alone triggers the current `needs_human` whole-URL `challenge` check. The visible page was the ordinary password form, with no visible CAPTCHA. Keep structural challenge detection separate from arbitrary URL substrings; preserve real CAPTCHA safety and login/consent classification. This issue was localized/reported, not fixed or reverified by Joy.