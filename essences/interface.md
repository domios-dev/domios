# Interface conventions

Domios has many surfaces built by many agents: chat, the side panel, the sidebar, Plugins, Settings, Memory, Jobs and Blueprints, notifications and dialogs. They read as one product only when they share a vocabulary. These rules are that vocabulary. A new surface follows them, and a change that touches an existing surface leaves it closer to them.

## Rules

1. **Names, not identifiers.** Primary text is what a person calls the thing: a tab title, a plugin's display name, a provider's name. Ids, absolute paths and CLI command paths never lead a row, card or header. They may appear as a tooltip, a muted secondary line in monospace, or inside a disclosure. When no name is known, the id is the last-resort fallback. A closed or missing entity is described in plain words ("a closed tab"), never by its id.
2. **One action hierarchy.** Buttons come from the shared button set: at most one primary action per surface, quiet (ghost) buttons for row actions, and the danger style only for the destructive confirmation itself. Tags and counts use the shared chip. A surface never invents its own pill or bordered inline button.
3. **Forms read top to bottom.** A field's label sits above its input; help text sits below it in muted type. A command a person might run belongs in a copyable code block inside a disclosure, never in a paragraph of prose.
4. **Time says what it measures.** An age reads as an age ("updated 5m ago"), a duration as a duration ("ran 3m 10s"), a future time as a future ("next in 2h"). A bare "5m" is not allowed, because it cannot be told apart from the other two. The exact timestamp is available on hover.
5. **Counts are compact.** Display counts as 999, 1k, 1.2k, 12k, 1.2M, with the exact number on hover. An exact number is shown in full only where a person edits it.
6. **Status is a word with a dot.** A state is shown as its word with a tone dot or tone chip. Colour never carries a status alone.
7. **One material.** Surfaces use the liquid surface set and the `--ct-*` theme tokens for colour, border, radius and motion. A literal colour appears only as the fallback inside a token reference or in a canvas palette that reads its values from the tokens.
8. **Empty and failed states direct.** An empty or failed state shows one short title and one muted line that says what to do next. It never shows a raw error code or an unstyled paragraph.

9. **Transient feedback belongs to the user's failed action.** A toast appears only when an action the user just took in this window failed and has no other visible failure surface. System, restore, Worker and automation notices stay in logs. Agent `notify send` entries belong to the bell and never synthesize an in-app toast. Local toasts never enter the bell. Eligible failures share one compact stack beneath the titlebar bell, newest first, with at most three visible. Each has a close button and expires after at most fifteen active seconds; hover pauses the countdown and reduced motion keeps the ring static. Inline errors, message delivery state and persistent progress remain in their own surfaces.

## Design language

The interface must feel quiet but alive: one continuous workspace, calm content, and physical responses that make actions legible. New components inherit the shared language. Review rejects visual fragmentation even when an isolated view looks polished. These sections refine the material and action rules above; they apply to both desktop and web shells described in the [core essence](core.md).

### One workspace surface and floating glass

Structural surfaces paint `--ct-bg`: chat, top bars, Jobs headers, the composer capsule, sidebar and its phone drawer. Depth comes from floating geometry and shadow. A new panel colour or component background constant that splits the workspace is rejected. The titlebar retains its restrained `--ct-titlebar-bg` derivation at +4%; flat cards and pills retain their subtle foreground mixes, and the inset composer field and user's bubble retain their distinct shared treatments. Header edges use the shared fade where applicable rather than adding stacked hard dividers. Sources: `src/lib/themeCss.ts`, `src/components/Terminal/tabTopBar.css`, `src/components/Terminal/composerDock.css`, and `src/components/Sidebar/sidebarPanel.css`.

Glass belongs to overlays above content: menus, popovers, compact-rail peeks and the floating phone context and Stop controls. It combines softened background colour, saturation, an almost opaque body, a thin top highlight and capsule lift. Consume `--ct-glass-blur`, `--ct-glass-saturate`, `--ct-glass-bg`, `--ct-glass-sheen`, `--ct-glass-edge` and `--ct-shadow-capsule` together. The shared material defines 40px blur, 1.6 saturation, the surface/background body mix and restrained sheen/edge; callers do not duplicate that recipe. Chrome pills and in-content cards stay flat. Layout capsules use `--ct-bg` and the capsule shadow without becoming glass. Sources: `src/styles/tokens.css`, `src/components/shared/dropdownMenu.css`, `src/components/Sidebar/sidebarPanel.css`, `src/components/Terminal/composerContext.css`, and `src/styles/agent-state.css`.

A glass surface must retain its backdrop blur through its interaction states and animations. Use the shared unprefixed `backdrop-filter` treatment; do not add a duplicate prefixed declaration with token values or apply `filter` to the glass element, including inside keyframes. These treatments must preserve the shared material rather than disabling its blur.

### Motion communicates action

Motion stays small and physical. `--ct-ease-liquid-settle` makes arrivals, reveals and position changes arrive quickly and settle softly. Hover pairs `--ct-duration-liquid-hover` with `--ct-ease-hover-swell` for a modest lift or swell; press uses `--ct-duration-liquid-press` for a brief squash. Structural changes use `--ct-duration-liquid-toggle`, and page changes inside one surface use `--ct-duration-liquid-flip`. Shared segmented, sweep, donut and breathing motions keep their corresponding liquid tokens rather than acquiring per-view timing recipes.

The base timings are 380ms for hover and toggle, 280ms for press and 360ms for flip. Reuse those tokens and each primitive's restrained geometry. Motion must not become a requirement for reading status or operating a control. Reduced motion preserves final layout, state, focus and functionality while removing or making movement instant. Sources: `src/styles/tokens.css`, the shared interactive rules in `src/styles/globals.css`, `src/components/shared/dropdownMenu.css`, `src/components/shared/rowKebab.css`, and `src/components/Terminal/composerContext.css`.

### Shared roles and ownership

Reuse the primitive that already expresses a role. A plain action or choice list uses `DropdownMenu` or `DropdownSelect`; compact chrome controls use `ct-bar-pill`; content cards and list rows use `ct-card`; overflow uses `KebabGlyph`; floating capsules share `--ct-shadow-capsule`; motion uses the liquid tokens. Parallel CSS that recreates a shared look is a defect to repair, never precedent.

A new shared primitive is justified when at least two consumers need the same role and an existing primitive cannot express it naturally. Introduce it shared and adopt it in those consumers together. A shared token is justified when separate primitives must agree on a value. One concept has one size source: callers consume the token or the primitive rather than copy its current number. Internal card padding, radius and icon geometry belong to the card, not to each caller. A card caller owns its width and supplies its tone or supported modifiers.

Source vocabulary: `src/components/shared/DropdownMenu.tsx`, `src/components/shared/card.css`, `src/components/Terminal/tabBarPill.css`, `src/components/shared/RowMenuButton.tsx`, and `src/styles/tokens.css`. `src/styles/globals.css` brings the shared styles into the product. Source names identify the contract's vocabulary; an existing divergent view does not create an exception.

### Menus, chrome and content

All menus and popovers must read as one family. Plain action lists use the shared dropdown behavior. A form, status readout or multi-step picker may need a dedicated panel, but it still uses the `ct-menu` material and applicable `ct-menu-row` parts. It does not invent another menu surface. `ct-menu-page`, separators, headings, identity blocks and back rows retain the same rhythm.

Menus fit the viewport and scroll internally when capped. Keyboard users can traverse rows, enter and leave submenus, close the root with Escape and recover trigger focus. Selection and independent toggles retain their radio/checkbox semantics; an unavailable choice remains inert and can explain why. A submenu keeps the root width, and its description is accessible without lengthening the visible header. The shared dropdown coordinates which menu is open. Sources: `src/components/shared/DropdownMenu.tsx` and `src/components/shared/dropdownMenu.css`.

Compact chrome uses the flat `ct-bar-pill` fill, with `--ct-bar-pill-tone` as its tone knob. A grouped control uses `ct-bar-pill-group`: one capsule around its children, with shared internal separators. Do not add a gradient or separate borders around each segment. Top-bar controls, Jobs tools and tool-card actions share this role. Sources: `src/components/Terminal/tabBarPill.css`, `src/components/Automation/styles.ts`, and `packages/chat-ui/src/toolcard/ToolCardFrame.tsx`.

Content uses the calm `ct-card` surface and its `__head`, `__icon`, `__title`, `__meta` and `__body` parts where applicable. `--ct-card-tone` accents the icon and subtle hover response, rather than painting the whole card. `ct-card--quiet` softens a collapsed row; `ct-card--error` uses a neutral dashed border while the status identifies the failure. A caller must not introduce coloured error fills or inner dividers. The user's `ct-user-bubble` has an intentional accent fill and directional tail; its relayed variant remains neutral. Sources: `src/components/shared/card.css`, `packages/chat-ui/src/ChatNoticeCard.tsx`, `packages/chat-ui/src/codeterm/ReminderCard.tsx`, `packages/chat-ui/src/toolcard/ToolCardFrame.tsx`, and `src/components/Jobs/JobRow.tsx`.

Overflow uses the shared vertical SVG `KebabGlyph`, with `RowMenuButton` for the standard trigger. Text ellipses, text `⋮` and alternate overflow icons are rejected. In tab rows the overflow target has its own trailing lane; it must not cover badges or status. Choice and context controls retain their appropriate triggers. Sources: `src/components/shared/RowMenuButton.tsx`, `src/components/shared/rowKebab.css`, and `src/components/Sidebar/tabRow.css`.

### Capsules and smaller screens

The sidebar and composer are floating capsules in the same workspace. They share capsule elevation and remain recognisable in the phone drawer. The sidebar stays reachable above main-content overlays. Resizing can settle into a useful full sidebar, compact rail or collapsed edge. A collapsed edge retains a discoverable peek and a way to restore the sidebar.

The compact rail shows General agents and the selected workspace's Team anchors rather than squeezing the whole workspace tree into icons. A peek unfolds the real tab row, preserving its identity and state. Mesh marking belongs only to mesh tabs. Sources: `src/components/Sidebar/Sidebar.tsx`, `src/components/Sidebar/SidebarCompactRail.tsx`, `src/components/Sidebar/sidebarPanel.css`, `src/components/Sidebar/TabItem.tsx`, and `src/components/Sidebar/tabRow.css`.

The composer field row and its round controls agree through `--ct-composer-control-size`; inner controls use `--ct-composer-inner-control-size`. Lane, gutters, safe-bottom spacing and radius use the corresponding composer tokens. Touch sizing changes at the shared sizing boundary without creating a second component design. Source: `src/components/Terminal/composerDock.css`.

On phone-width shells, the NarrowExpand presentation starts with the context header folded. A glass pill above the composer shows the plan or linked kind, linked count, expand affordance and attention indication when needed. Tapping unfolds the existing header and its kind strips inside the capsule; starting a draft folds it again. Stop remains a reachable round control on the opposite side. Desktop retains the inline header. This is one context component with responsive presentation, not a duplicate phone implementation. Sources: `src/components/Terminal/ComposerContextHeader.tsx`, `src/components/Terminal/composerContext.css`, `src/styles/agent-state.css`, `src/hooks/useSidebarViewport.ts`, and `src/lib/shellBreakpoint.ts`.

### One way back

A view or detail has one way back: the shared `BackPill`, a `ct-bar-pill` with a chevron and visible "Back". Its accessible name identifies the destination. Returning follows the place the view was opened from, represented by `returnTo`, rather than forcing a visit to an intermediate hub. Jobs opened directly in Settings leaves Settings when the user returns.

Do not stack a host header and view header that repeat the title, add a close X to a view that already has Back, or leave list filters, import and clear controls above an open detail. The detail owns its header and the list header is hidden. The sidebar remains available for wider navigation. Sources: `src/components/shared/BackPill.tsx`, `src/lib/returnTo.ts`, `src/components/Jobs/JobsView.tsx`, `src/components/Cron/CronWorkspaceView.tsx`, `src/components/Automation/ScenarioPanel.tsx`, and `src/components/Settings/SettingsPanel.tsx`.

### Kind and status tones

A Blueprint, cron or delayed message keeps its kind tone across Jobs, composer context and chat deliveries. Consume `--ct-tone-blueprint`, `--ct-tone-cron` and `--ct-tone-delayed`, respectively. Their theme-relative definitions are an accent/foreground mix, yellow and green; the shared token owns the exact colour. Kind identity must remain distinct from active, scheduled, waiting, failed or stopped state.

Use kind tone in icons, icon tiles and restrained hover accents. Composer strips may use very faint Blueprint grids, cron dots and delayed diagonals, tuned per pattern without becoming a visible fill. Full kind-coloured card fills, resting accent rails and loud borders are rejected. Status remains understandable through the status convention above. Sources: `src/styles/tokens.css`, `src/components/Automation/styles.ts`, `src/components/Jobs/JobRow.tsx`, and `src/components/Terminal/composerContext.css`. These rules also govern deliveries as they adopt the shared cards.

### Scrolling

Bounded Chat message bodies and reports initially clip at their preview height without an inner vertical scrollbar. Show all raises the limit by 30% and allows internal scrolling when needed. Wheel, trackpad and touch continue into the transcript at an expanded body’s edge. The transcript retains its outer overscroll boundary and explicit jump to latest on desktop and mobile web.

Every ordinary scrolling view inherits the shared scrollbar. It is transparent at rest and appears during scrolling, with a narrow rounded thumb derived from the current background. Hover alone must not reveal a separate per-view scrollbar. `--ct-scrollbar-thumb` owns its colour; `data-scrolling` represents scroll activity.

Do not set `scrollbar-width` or `scrollbar-color` in a view: on Chromium they override the shared WebKit styling. The shared fallback for browsers without that styling owns those properties. Intentional hiding in horizontal tab strips and the xterm viewport is allowed; xterm's own slider keeps the same thumb colour and visible width. Sources: `src/styles/primitives.css`, `src/lib/scrollActivity.ts`, `src/main.tsx`, and the xterm and `ct-hscroll` rules in `src/styles/globals.css`.

### Measured rhythm

Spacing is deliberate. Components inherit a shared role's geometry and use its exposed size tokens; arbitrary per-view approximations are rejected. Preserve alignment between field and button heights, card title and body, menu glyph and row, and badge and overflow lanes. When two primitives must agree and no shared token exists, establish that token at their common home and migrate the consumers together.

The measurements below identify the shared definitions to consume. They are the agreed measured rhythm, not numbers to duplicate in new callers. Preserve them and the relationships above unless the design authority consciously revises the rhythm.

| Role | Shared measurement | Owning vocabulary |
|---|---|---|
| Controls | `--ct-control-height` 2.125rem; small 1.875rem; micro 1.625rem in the base token set | `src/styles/tokens.css` |
| Composer controls | Outer size aliases control height, inner aliases micro; touch overrides are 2.5rem and 2rem | `--ct-composer-control-size`, `--ct-composer-inner-control-size` in `src/components/Terminal/composerDock.css` |
| Menu | 14px outer radius, 5px inset; rows at least 30px, 9px row radius, 15px icon slot | `ct-menu`, `--ct-menu-row-radius`, `ct-menu-row` in `src/components/shared/dropdownMenu.css` |
| Chrome pill | `--ct-tab-pill-h` with 24px fallback, 27px in the top bar; 10px side padding, 6px gap | `ct-bar-pill` in `src/components/Terminal/tabBarPill.css`; `src/components/Terminal/tabBarConstants.ts` |
| Content card | 14px radius; head inset 9px by 12px; 26px icon tile; body left inset 48px | `ct-card` parts in `src/components/shared/card.css` |
| Capsules | Composer radius 1.215rem, bottom-bar radius 1.575rem, gutter 0.75rem; sidebar inset 0.5rem and radius 1.25rem | Composer tokens in `src/components/Terminal/composerDock.css`; `src/components/Sidebar/sidebarPanel.css` |
| Sidebar rail and row | Rail width 64px and icons 36px; desktop tab row reserves a 1rem trailing overflow lane | `src/components/Sidebar/Sidebar.tsx`, `sidebarPanel.css`, and `TabItem.tsx` |
| Overflow | 16px by 22px base target; 44px square touch target; 4px by 14px base glyph | `ct-row-kebab`, `KebabGlyph` in `src/components/shared/rowKebab.css` and `RowMenuButton.tsx` |
| Scrollbar | 4px widget with transparent 1px borders yields a 2px visible thumb; background/foreground mix 80/20 | `--ct-scrollbar-thumb` in `src/styles/primitives.css` |

### Gallery as a design contract

The gallery presents actual product components with actual product styling. Specimens import from `src/` or `packages/` and load `src/styles/globals.css`; copied components, a parallel design-system stylesheet, or a mock of the rendered component are rejected. Fixtures may replace environment and data boundaries, including IPC, Tauri, viewport conditions and the portal host.

A primitive's review presentation includes every meaningful state it supports: rest, hover, focus, press, open, selection, disabled or unavailable, and error as applicable. Responsive primitives also show the narrow or phone presentation where behavior changes. Motion is shown live and reduced-motion behavior remains legible. A gallery screenshot or source import alone does not establish interaction quality or complete coverage. Sources: `playground/src/main.tsx`, `playground/vite.config.ts`, `playground/src/gallery/Gallery.tsx`, and `playground/src/gallery/specimens/`.

### Review rejects

- Hand-rolled action menus, copied dropdown behavior or bespoke popover materials.
- Duplicate back/close controls, repeated headers or list controls above a detail.
- Per-view scrollbar skins or visibility rules.
- Duplicated base surfaces, independent capsule shadows or loud card fills.
- Hard-coded caller sizes that replace an existing token, or two size sources for the same control concept.
- Parallel pills, cards, overflow glyphs or motion recipes that repeat a shared role.
- A gallery specimen that copies the component or styling instead of exercising the product.

Existing deviations require implementation repair. They do not weaken this contract or authorise rewriting intended behavior around a bug.


## Recurring violations

The table records the classes of inconsistency found in a sweep of every surface, so the same mistakes are recognised the next time they appear.

| Surface | Problem | Rule |
|---|---|---|
| Chat command rows | A `codeterm plugin` call rendered as its CLI path and the plugin id ("plugin get lmstudio") | 1 |
| Chat message origin | A plugin's message header fell back to the plugin id when the message carried no label | 1 |
| Chat removed-plugin state | Two unstyled paragraphs, with the plugin id as the headline | 1, 8 |
| Settings, agent providers grid | Every provider card's subtitle was its id, repeating the name in machine form | 1 |
| Settings, Supporter activity | Request counts printed in full locale format | 5 |
| Sidebar team terminals | "started by" showed the creator's tab id once that tab closed | 1 |
| Sidebar team crons | The report target was named by tab id | 1 |
| Plugins page | Plugin rows carried raw CLI paragraphs | 1, 3 |
| Memory | Ages and durations shared one unlabelled format | 4 |
| Canvas, Usage, Mesh, Titlebar | Literal colours and tracked upper-case labels outside the token set | 7 |
| Plan boards, Usage, delayed sends | Several separate relative-time formatters with different wording | 4 |

## Guards

Where a rule can be checked cheaply it is checked mechanically: bespoke chips and inline buttons, and literal colours on each surface, are counted against ratchets that may only fall; ad-hoc spinners and "Loading…" strings are rejected, and every plugin subcommand known to the CLI must render its target by display name in chat. A rule without a guard is enforced by review against this document.

```map
anchors:
  - client:ui
  - capability:chat-view
  - capability:web-ui
  - capability:plugins-and-channels
  - capability:notifications
```
