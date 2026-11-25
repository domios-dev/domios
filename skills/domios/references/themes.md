# Themes

`domios docs RUNTIME-CONFIG` lists the built-in themes, the `domios theme` commands and the custom theme TOML format; `domios docs API-TABS` has the REST routes.

For a wallpaper request in ordinary chat, read `domios docs APPEARANCE`, create a complete `domios appearance propose` look and return its native `<ds-appearance>` card so the owner can apply it. Follow the same path after an owner-only `wallpaper set` or `wallpaper clear` refusal. The card includes **Use theme wallpaper** when an override hides the theme backdrop.

If the target General session receives a one-time wallpaper draft request ID, it can submit the static SVG from its pad:

```sh
domios wallpaper set --request REQUEST_ID --from-pad tile.svg
```

<!-- revision: 3 -->
