# External design-system drop audit

Audited `C:/Users/dev/AppData/Local/Temp/claude/ds-zip/design-system` against the
current product source. This is a source audit, not a visual approval.

## Tokens

| Area | Drop | Product source | Verdict |
| --- | --- | --- | --- |
| Built-in theme palettes | Eight palettes and the generated chrome aliases match the current built-ins (for example GitHub Dark `#0d1117`, `#c9d1d9`, `#58a6ff`). | Runtime generation in `src/lib/themeCss.ts` from `src-tauri/themes/*.toml`. It also emits syntax/diff foreground variables and supports arbitrary custom themes. | **Adapt, not copy.** `tokens/themes.css` is a static snapshot and its `:root` GitHub Dark block can override runtime/custom `--ct-*` values by import order. |
| Core type/control/motion | `Inter`, `JetBrains Mono`; 13/14/16px; 26/30/34px controls; 50/120/200/320/480ms and all five curves are exact. | `src/styles/tokens.css`. | **Adopt source references only.** The shared values are accurate. |
| Spacing/chrome sizes | Adds 4/6/8/10/12/16/20/24px scale, 36px titlebar, 40px toolbar, 28px rows. | No equivalents in `src/styles/tokens.css`; real layout values remain local classes/CSS. | **Ignore as tokens.** Plausible inventory, but falsely described as verbatim product CSS. |
| Radii/elevation/glass | 4/8/12/16/99px, shadows, blur, scrim, focus/text opacity match. | `src/styles/tokens.css`. | **Adopt source references only.** `--ct-radius-pill` is additive; `--ct-scrim` aliases product `--ct-bg-overlay`. |
| Semantic aliases | `ok/err/warn`, derived surface/text/line/accent fills. | Product has only base/runtime variables; no matching semantic layer. | **Conflict.** Some aliases are useful but `color-mix()` recipes are an unaudited new layer, not product tokens. |
| Static generic defaults | Drop uses generic fallbacks such as `#5b8cff`, `#e8e8ef`, 6px radius, 1.55rem control. | Product defaults are 4px radius and 30/34px controls; dynamic theme supplies colors. | **Ignore.** These change the product where a dynamic var is absent. |
| Fonts | Google-hosted imports. | Product declares family stacks but does not load hosted font CSS in `src/styles/tokens.css`. | **Conflict.** Network/font-loading policy is invented; do not import. |

The color constants in `tokens/colors.css` are correct for graph lanes, Bitwarden,
Git, and thinking (`#a371f7`). Their phase-dot copy is deliberately not a salvage
candidate: `src/styles/agent-state.css` is the product source and has the better
reduced-motion/idle behavior.

## Component inventory and fidelity

The playground currently has only `AgentTreeRow`, `AgentStatusRow`,
`PlanEntryCard`, `WorkspaceCubeGrid`, and the liquid-glass study. The drop fills
generic primitive gaps, but it is not a faithful export of the application.

| Drop component(s) | Playground gap / product analogue | Fidelity to product | Verdict |
| --- | --- | --- | --- |
| `controls/Button.jsx` | Gap; `src/components/shared/Button.tsx` | Partial: matches class names/variants, but defaults to secondary rather than product ghost, omits `icon`, `compact`, `loading`, `aria-busy`, and injects divergent CSS. | **Adapt only** from the real Button; do not copy. |
| `Checkbox`, `Input`, `Textarea` | Gaps; product uses native fields plus `UI_INPUT` (`src/components/shared/design.ts`) at call sites. | Invented generic wrappers; checkbox's Git staging story has no corresponding current Git UI. | **Discard.** |
| `IconButton` | Gap; product uses Button's `icon` variant and individual accessible controls. | Partial visual resemblance, but wrong default square sizing and no accessible-name contract. | **Adapt only** if a real shared primitive is later approved. |
| `Chip`, `Meta` | Chip exists in `src/components/shared/Chip.tsx`; meta text is local. | Chip's Git-ref tones and Meta's global styling are invented; product Chip has its own API. | **Discard.** |
| `DeltaPill`, `StatusBadge` | No playground primitive; product has unrelated worker/watcher status badges. | Invented from a nonexistent current Git diff/worktree surface. | **Discard.** |
| `Spinner` | Gap; product `BusyState`/`BusySpinner` in `src/components/shared/BusyState.tsx`. | Dot keyframes were copied from `src/styles/agent-state.css`, but implementation omits the product reduced-motion/idle behavior and exposes an incompatible API. | **Discard** (per lead direction); use product BusyState if needed. |
| `Dialog`, `Menu`, `Toolbar` | Gaps; product has specialized portals/menus (`DropdownMenu.tsx`, `TabActionsMenu.tsx`, `SettingsMenu.tsx`). | Generic visual approximation. Dialog lacks focus trap, Escape/outside semantics and portal; Menu lacks keyboard roving/focus management; Toolbar is only a speculative layout. | **Discard.** |

All drop JSX files self-inject `<style>` elements. That duplicates and competes
with `src/styles/globals.css`, bypasses Tailwind conventions, and makes the
drop unsuitable for direct import into the playground.

## Assets and provenance

| Asset | Result | Evidence / provenance finding |
| --- | --- | --- |
| `assets/logo.png` | **Byte-identical; safe reference only.** | SHA-256 `e81e4be187c12379d0788c200e16b692eed0964142b288c6e5eb679a842e9589`, identical to `src/assets/logo.png`. Do not duplicate it. |
| `assets/icon.svg` | Not an app asset. | SHA-256 `5f5794d8…`; generic Bitwarden-style shield markup. |
| `assets/icons/{bitwarden,chat-preprocessor,git,lmstudio,transcriber}.svg` | **Not byte-accurate to anything in this repository or `plugins/`.** | Repo plugin icons are runtime `iconSvg` manifest fields (for example `plugins/chat-preprocessor/plugin.json`), not these files. The drop uses generic/common SVG paths (shield, sparkles, git graph, robot, microphone), so they could readily be inferred from plugin ids or generated from an icon library. Provenance is unverified. |

## Salvage and discard

**Salvage later by adaptation:** the token inventory as a comparison checklist;
the exact shared type/control/motion values; the byte-identical logo reference;
and the component names as a backlog for playground specimens. If primitives are
added, derive `Button` from `src/components/shared/Button.tsx` and loading from
`BusyState.tsx`, without self-injected CSS or static theme blocks.

**Discard:** `tokens/themes.css`, `tokens/fonts.css`, semantic recipes, all
direct-import JSX components, phase dots, and all extension icon assets. The
highest risks are runtime-theme override, invented Git-specific UI, lost
accessibility/interaction behavior, and unproven icon provenance.

<!-- revision: 1 -->
