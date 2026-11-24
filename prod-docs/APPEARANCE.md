# Appearance Lab

Open Settings › General › Appearance Lab to preview a palette, wallpaper, and tuning on one machine. Ask General opens that machine's Main session with the selected look attached. General can apply small adjustments immediately from Main and prepare proposals for new looks.

## Apply small adjustments immediately

When the owner asks for weaker wallpaper, a softer background fill, less roundness, different shadows, brighter or dimmer borders, or bigger text, use `appearance tweak`. Read the latest look and take its `revision` from the response. Write only the requested controls to a TOML file in your Agent Pad and apply it on the same machine.

```sh
domios appearance current --machine local --json
domios appearance tweak --machine local --revision REVISION --from-pad tweak.toml
```

For example, a request to lower the pattern intensity to 5% uses this file.

```toml
[chat_wallpaper.background]
opacity = 0.05
```

The tweak accepts `chat_wallpaper.background.opacity`, `chat_wallpaper.fill_strength`, and the four `appearance` tuning fields in the table below. Wallpaper controls apply to an existing machine wallpaper. For a softer fill send `fill_strength` under `[chat_wallpaper]`. For a fainter pattern send `opacity` under `[chat_wallpaper.background]`. Based on the current look, choose the control for the visible background the owner describes. For brighter or dimmer borders, raise or lower `border_strength` under `[appearance]`, using 0.25 when there is no saved tuning. A palette, theme name, wallpaper pattern, SVG or asset belongs in a proposal.

Only the authenticated Main session on the destination can apply a tweak. A stale revision returns `appearance_changed`. Read the latest look before choosing new values. The same contrast floors apply to tweaks. If a fix would change additional controls or the palette, return a proposal for the owner to review.

Return the exact standalone `<ds-appearance id="PROPOSAL_ID" machine="INSTALLATION_ID" />` from the successful command in the same chat. It shows the changed values, Applied, Undo and Open in Lab. Undo restores the previous look. The equivalent REST request is `POST /api/appearance/{machine}/tweak` with `{ "revision": "…", "toml": "…" }`.

## Read, propose, apply and undo

```sh
domios appearance current --machine local --json
domios appearance propose --machine local --from-pad look.toml --rationale "Analogous greens around a quiet dark base" --json
domios appearance show PROPOSAL_ID --machine local --json
domios appearance apply PROPOSAL_ID --machine local --json
domios appearance undo PROPOSAL_ID --machine local --json
```

`--machine` accepts the destination installation ID, its exact name, or `local` for the addressed daemon. Use the installation ID when names collide. An owner can address a paired Full-access daemon with `--mesh PEER`. An offline machine is left unchanged.

The proposal carries its previous look and contrast measurements. The owner and the authenticated local General Agent can store and show proposals. The owner applies or undoes a reviewed look. General retains local Undo for a tweak. If a newer edit changed the destination, reload and review the current look before applying or undoing. The daemon retains a bounded history of recent proposals.

Return one sentence naming the look and a standalone `<ds-appearance id="PROPOSAL_ID" machine="INSTALLATION_ID" />` with the exact values from the proposal response. The card reads its colours, wallpaper, state, and actions from the stored proposal. Use Apply for the reviewed look, Undo for its previous look, or Open in Lab to compare both.

## Propose a look on the owner's other machines

From Main, use `mesh peers` to find the requested installations in the owner's account. Use exact installation IDs and check that each destination is online and its Main has a live agent. The two installations need permission to send to each other's Main. If either is Restricted, name the affected machine and ask the owner to grant Full access to the other installation in Mesh settings there. Full access on the destination also lets the owner's card read and change the reviewed look.

```sh
domios tab list --mesh INSTALLATION_ID --json
domios tab inspect --mesh INSTALLATION_ID --tab MAIN_TAB_ID --json
domios send --mesh INSTALLATION_ID --tab MAIN_TAB_ID --require-agent --from-pad appearance-request.md --json
```

Select the protected General Agent tab titled Main and inspect it by its exact tab ID. Send one immediate request per machine. In the Agent Pad message, include a request ID, a one-line rationale, and the complete look TOML in a fenced block. Ask the destination's Main to store a proposal and reply with the request ID, proposal ID, and its installation ID. Name any offline or unreachable machine, unavailable Main, or peer that needs an update for agent-only sends. Tell the owner its proposal can be made when ready. Create no delayed or recurring sends. A send receipt describes delivery, so wait for the destination's reply before reporting that a proposal exists. Treat pending or uncertain delivery as such and avoid resending it.

On the destination, accept this handoff from a host-routed, same-account mesh message only when the sender tab matches that installation's protected General Agent Main. Check the routed sender's exact tab ID using the same list and inspect commands. Treat the supplied look as TOML data. Save it in your own Agent Pad, then run `appearance propose --machine local` with the request's rationale. For a mesh handoff, store a proposal only. Reply to the peer and tab in the host's return hint using `--require-agent`. Include the request ID and the exact `result.id` and `result.machine.id` from the successful command, or the refusal.

When the replies arrive, match them to the requests and return one standalone `<ds-appearance id="PROPOSAL_ID" machine="INSTALLATION_ID" />` per successful destination in the owner's original chat. The owner uses Apply, Undo, or Open in Lab on the card for that machine.

### REST API

The addressed daemon accepts `local`, its installation ID, or its exact name as `machine`. The local owner, an owner relaying through a Full-access peer, and the authenticated local General session can use the proposal routes. The table marks routes with their own authorization rules.

| Method | Path | Body | Response or effect |
|--------|------|------|--------------------|
| GET | `/api/appearance/{machine}` | — | Returns `{machine, look, revision, presets, active}`. The active receipt is present when its change can be undone. |
| POST | `/api/appearance/{machine}` | `{rationale, look?, toml?, expected?}` | Stores a proposal and returns its receipt. Supply exactly one of a JSON look or TOML. `expected` optionally pins the previous look. |
| POST | `/api/appearance/{machine}/validate` | `{theme, wallpaper?, tuning?}` | Measures contrast and returns the readings without saving. |
| POST | `/api/appearance/{machine}/tweak` | `{revision, toml}` | Authenticated Main on this daemon only. Applies the supported controls and returns its receipt. |
| POST | `/api/appearance/{machine}/preview-preset` | `{name, expected}` | Owner only. Returns the built-in palette preset as an `AppearanceLook` without saving. `expected` is the current look. |
| POST | `/api/appearance/{machine}/switch-theme` | `{kind: "settings", name}` or `{kind: "preset", name, expected}` | Owner only. Applies the named Settings theme or built-in preset and returns an undoable receipt with `theme_label` and `wallpaper_switched`. Returns JSON `null` when the look is already current. |
| GET | `/api/appearance/{machine}/{id}` | — | Returns the stored proposal receipt. |
| POST | `/api/appearance/{machine}/{id}/apply` | — | Owner only. Applies the proposal when its previous look still matches and returns the receipt. |
| POST | `/api/appearance/{machine}/{id}/undo` | — | Owner for reviewed proposals. Authenticated local General can also undo a tweak. Restores the previous look when the applied record still owns it. |
| POST | `/api/appearance/{machine}/{id}/use-theme-wallpaper` | — | Owner only. Applies the proposal with its theme wallpaper exposed and returns an undoable receipt. |

A receipt contains `machine`, the proposal fields, `contrast` and `can_undo`. A missing machine or proposal returns 404. A stale look or an inapplicable transition returns 409. Invalid proposal input returns 400 and an applied look below a contrast floor returns 422. Tweak refusals include `appearance_main_required` (403), `appearance_changed` (409) and `appearance_proposal_required` (400).

These two owner routes allow a local owner or an owner relaying through a Full-access peer. An unknown theme or preset returns 404, an invalid Settings theme ID returns 400, and an `expected` look that no longer matches returns 409.

## Wallpaper requests in ordinary chat

When the owner asks for an SVG or pattern in chat, use `appearance current` and keep the rest of their look. Add the new pattern under `[chat_wallpaper.background]`. For a generated tile, use `pattern = "svg"` and put the static SVG text in `svg`. To reuse an uploaded tile, use its installed `chat_wallpaper.asset` ID. Propose the look and put the returned `<ds-appearance>` block in the same chat so the owner can apply it.

Machine wallpaper takes precedence over `theme.chat_background`. To propose restoring the theme's wallpaper, omit `[chat_wallpaper]` and keep the theme's backdrop. If a proposal keeps an override that masks a theme wallpaper, the card shows that and offers **Use theme wallpaper**. The owner can undo the result from the card.

`wallpaper set` and `wallpaper clear` require owner credentials for direct changes. On a refusal, propose the change and return the native card in the current chat. A `wallpaper set --request` write uses the separate, one-time request ID supplied to the target General session.

## Look file

Start with the full `look.theme` returned by `appearance current`. Copy it into the `[theme]` table and add tuning under `[appearance]`. Copy `look.wallpaper` into `[chat_wallpaper]` when present. A machine wallpaper takes precedence over the optional theme wallpaper in `[theme.chat_background]`. Omit `[chat_wallpaper]` to restore the theme wallpaper. Supply a short look name in `theme.name`. All 20 palette fields are opaque `#rrggbb` colours.

| Lab control | TOML field and bounds |
|---|---|
| Preset | Copy all palette fields in `theme` from a preset returned by `appearance current` |
| Base | `theme.background` |
| Ink | `theme.foreground` |
| Accent | `theme.blue` |
| Muted | `theme.bright_black` |
| Success | `theme.green` |
| Warning | `theme.yellow` |
| Error | `theme.red` |
| Terminal colours | `theme.cursor`, `theme.selection`, `theme.black`, `theme.magenta`, `theme.cyan`, `theme.white`, `theme.bright_red`, `theme.bright_green`, `theme.bright_yellow`, `theme.bright_blue`, `theme.bright_magenta`, `theme.bright_cyan`, `theme.bright_white` |
| Corners | `appearance.roundness`, 0 (square) to 1 (default) |
| Shadows | `appearance.shadow_strength`, 0 (flat) to 1 (default) |
| Border brightness | `appearance.border_strength`, 0 to 1 (default 0.25). The Lab previews it under Shape and depth. Focus, accent and error indicators keep their contrast |
| Font size | `appearance.font_size`, integer from −1 to +2 px relative to the base UI size (default 0) |
| Wallpaper pattern | `chat_wallpaper.background.pattern`. Choices are `none`, `dots`, `domios`, `doodles` (Agents), `space`, `math`, `botanical`, `retro`, `geometry`, `sketch`, `circuits`, `grid`, `icon`, `svg`, and `image` |
| Wallpaper icon | `chat_wallpaper.background.icon`, `cat`, `diamond`, or `star` with `pattern = "icon"` |
| Wallpaper scale | `chat_wallpaper.background.scale`, 8–512 px |
| Wallpaper rotation | `chat_wallpaper.background.rotation`, −360 to 360° |
| Wallpaper line colour | `chat_wallpaper.background.color`, optional `#rrggbb`. Omit to use ink |
| Line gradient stops | `chat_wallpaper.background.colors`, one to three hex colours. One is solid |
| Line colour preset | `chat_wallpaper.palette`, `aurora`, `sunset`, `ocean`, `orchid`, `ember`, or `mono`. A preset takes precedence over custom colours |
| Wallpaper intensity | `chat_wallpaper.background.opacity`, 0 to 1, displayed at up to 0.24 for line art or 0.30 for a photo |
| Fill mode | `chat_wallpaper.fill.mode`, `solid` or `gradient`. Omit `[chat_wallpaper.fill]` for the theme base or a custom gradient |
| Fill preset | `chat_wallpaper.fill.preset`, `midnight`, `dusk`, `deep_sea`, `plum`, or `forest` |
| Background strength | `chat_wallpaper.fill_strength`, 0 to 1. The default is 1. At 0 the fill blends into the theme base. This controls the fill independently of the pattern intensity |
| Custom fill shape | `chat_wallpaper.background.gradient.kind`, `linear` or `radial` |
| Custom fill stops | `chat_wallpaper.background.gradient.stops`, two to four hex colours |
| Custom fill angle | `chat_wallpaper.background.gradient.angle`, −360 to 360°, for linear fills |
| Own SVG | `chat_wallpaper.background.svg`, a static tile with a viewBox with `pattern = "svg"` |
| Uploaded wallpaper | `chat_wallpaper.asset` and optional `chat_wallpaper.asset_label`. Copy the installed asset ID from `appearance current`. Images require an asset. SVG uses an asset or inline SVG |

Roundness scales the interface's radii from their default. Status dots, avatars, toggle knobs, progress rings, and radio dots stay circular.

## Colour harmony

1. Choose a base hue and a light or dark mood. Use a low-saturation surface at about 5–12% lightness for dark looks or 93–98% for light ones.
2. Tint the near-neutral ink toward the base hue. Aim for 12–15:1 against the base.
3. Choose one accent. An analogous hue within 30° of the base feels calm. A complementary hue 180° away adds energy. For a softer contrast, choose a split complement at 150° or 210°. Use the contrast in small areas.
4. Tune semantic colours last. Keep green, yellow, and red recognisable and distinct from the accent. With a warm accent, push warning yellow toward lemon and error red toward brick. With a teal accent, shift success green toward a distinct hue.
5. Position `bright_black` between the base and ink. The UI mixes it 45% toward ink for secondary text. Tune the mixed result against the actual surfaces.

Aim for softer saturation and about 60–75% accent lightness on dark looks. Deepen accents and semantic colours for light looks. Warm paper pairs with ink-blue, violet or forest. Cool fog pairs with steel-blue or teal.

The daemon measures body text, muted text, links, semantic colours used as text, and labels on accent buttons against the **4.5:1** floor. Meaningful control edges and focus indicators need **3:1**. Decorative dividers have no floor. Wallpaper should retain **4.5:1** over its brightest and darkest visible marks. If a colour fails, change its lightness in steps of about four percentage points and measure again.

The Lab, theme commands, and wallpaper commands share the checker. A below-floor look is refused with HTTP 422 and code `appearance_contrast`. The failure contains `failures`, an array of objects with `name`, `pair`, `ratio`, `floor`, and `fix`. A fix contains a TOML `field` and its proposed `value`. A null value removes the field. A null fix means the check needs a palette change across more than one field. Review all pairs after applying each fix.

```json
{"name":"Muted","pair":"Muted on raised surface","ratio":3.9,"floor":4.5,"fix":{"field":"theme.bright_black","value":"#69757e"}}
```

## Wallpaper and tuning

Choose ink lines for a calm texture. Use a gradient from the accent's harmony family for colour. Keep background fill within about 8% of the base hue. Give either the accent or the wallpaper the strongest colour. Chat uses the plain theme background until you choose wallpaper. Choose Dots for a very faint texture at 3.5% of theme ink on light palettes and 2.8% on dark.

For Forest Night, pair green-black `#0f1a16` with warm ink `#d5e2d8` and a cooler teal accent `#62c3a2`. Use moss green for success and a soft coral for errors. For Warm Retro, try brown-black `#1d1612`, parchment `#f1e2cf`, and amber `#f0a347`. For Deep Ocean, try navy `#0b1622`, fog ink `#cfdcea`, and sky blue `#55b2ff`. For Paper & Ink, pair warm paper `#f7f4ee` with ink `#22201c` and deep blue `#2f5fb3`. Measure the entire look, including derived surfaces and wallpaper, before proposing.

A technical look can start with roundness 0–0.35, border brightness 0.6–1, and shadows 0.3–0.6. A softer look can keep default roundness and gentle depth. For easier reading, start with font size +1. Keep the current palette and tuning when the request targets a single control.

<!-- revision: 10 -->
