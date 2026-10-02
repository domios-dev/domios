# Domios website

Standalone Vite, React, and TypeScript landing page. Run commands from `web/`:

```sh
npm install
npm run dev
npm run build
npm run preview
```

Build output is `dist/`. The Vercel project is `domios-web`, with `web` as its root directory and Vite as its framework preset. No app build, runtime service, or environment variables are required.

## Media

The media lane owns `public/media/`. Merge its commit with ancestry preserved before deploying. The site reads `/media/manifest.json` and `/media/captions.json`. Manifest shape:

```json
{
  "schemaVersion": 1,
  "clips": [{
    "id": "board",
    "mp4": { "src": "/media/board.mp4" },
    "webm": { "src": "/media/board.webm" },
    "poster": { "src": "/media/board-poster.webp" },
    "captions": [{ "start": 0, "end": 4, "text": "A team with a plan." }]
  }]
}
```

Clip ids: `board`, `mesh`, `plugins`, `mesh-talk`. A separate captions file is an object keyed by those ids, with an array of `{start,end,text}` per clip. It overrides inline captions. Without a manifest, conventional filenames above are used. Unavailable videos show a placeholder; missing files do not break navigation.

Only the active clip loads when in view. Playback pauses offscreen, in a hidden page, or by user request. Reduced motion disables animation and automatic video loading; a user can explicitly play a clip. Caption text is also available in a disclosure below the video.

## Visual checks

Inspect dark and light themes at 360px and desktop widths, keyboard tab selection (arrows, Home, End), theme persistence, reduced motion, and video pause/resume. The page motion preference follows the system. Confirm all four clips and captions after the media merge. No Playwright runs are needed.

Colors are ported from the application's GitHub Dark/Light palettes and `themeToCSS` derivations. Motion timings and liquid easing come from the application tokens and primitives. No code is imported from outside this directory.

## Build troubleshooting

If Vercel refuses more than 15,000 uploaded files, verify that `.vercelignore` is present in `web/`. The website keeps local recordings and review evidence under `.local/`; Git exclusions alone do not define the Vercel upload. The deployment allowlist includes only application source, public assets, API handlers, and build configuration. Keep private evidence and environment files excluded instead of archiving the entire working directory to bypass the limit.

The production build embeds the small landing-page stylesheet in HTML through `vite.config.mjs`, removing a render-blocking request on mobile. Development retains Vite's CSS hot reload. If styles disappear after changing the build pipeline, inspect the emitted HTML for its `<style>` tag and verify the stylesheet asset lookup against Vite's output bundle; do not replace it with delayed style loading.

If TypeScript rejects an `Object.fromEntries` cast to the media library with TS2352, the generated object's keys are not proven complete. Keep the explicit typed four-clip record and shared clip factory in `src/media.ts`; do not mask it with a double cast. Run `npm run build` again after the correction.

In Windows PowerShell, use `npm.cmd run preview -- --port 4173 --strictPort` when forwarding flags. The `npm.ps1` shim can consume those flags and pass the port as a positional directory, causing a misleading missing-`dist` error even after a successful build.
