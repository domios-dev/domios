# Domios website

Standalone Vite, React, and TypeScript landing page. Run commands from `web/`:

```sh
npm install
npm run dev
npm run build
npm run preview
```

Build output is `dist/`. The Vercel project is `domios-web`, with `web` as its root directory and Vite as its framework preset. No app build, runtime service, or environment variables are required.

## Publication

The Vercel project is connected to `rollacode/codeterm` through its GitHub integration. The production branch is `master`, which is this repository's default branch. Merging website changes into `master` triggers a production build and publication to `domios.dev`. `vercel.json` disables Git deployments for all other branches, including slash-named release and lane branches. The project runs `npm ci` and `npm run build` from `web/`, with `dist/` as its output directory. No separate GitHub Actions deployment workflow is needed.

The project's Ignored Build Step also skips non-production builds for branches that do not yet contain this configuration: `if [ "$VERCEL_ENV" = "production" ]; then exit 1; else exit 0; fi`. Exit 1 continues production; exit 0 cancels a preview before dependency installation. Such old branches can still create canceled deployment records. Manual production deployment remains available.

The selected trailer's recordings, narration, music, stems and portable rebuild instructions are preserved in [media-src/projects/domios-memory-2026-10-03](media-src/projects/domios-memory-2026-10-03/README.md). These source files are outside `public/` and the built site.

For a manual publication, redeploy the reviewed Git reference through the Vercel project with the production target. The former CLI upload from a flat `web/` directory predates the Git connection: do not reuse that upload layout with the project's `web` root setting. A CLI source upload must preserve the repository's `web/` directory, or use a correctly generated prebuilt deployment.

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

If Git deployments fail at `npm ci` with a missing lockfile after logging removed ignored files, inspect `.vercelignore`. The former flat-directory `/*` allowlist excluded `web/package-lock.json` when applied to the repository checkout. The current rules exclude private/generated directories at any depth and preserve build inputs in both path layouts. Verify that the selected Git revision contains `web/package-lock.json`; do not regenerate a valid lockfile to repair an exclusion error.

For manual CLI uploads, stage only tracked files from `web/api`, `web/src`, `web/public`, plus `web/index.html`, `web/package.json`, `web/package-lock.json`, `web/tsconfig.json`, `web/vite.config.mjs`, and `web/vercel.json`. Preserve the `web/` prefix and project link in the staging root. Never upload the whole working tree: `.local`, `media-src`, environment files and installed dependencies are not deployment inputs. Verify the staged paths before deploying; a 15,000-file refusal means the selection is wrong, not that the entire tree should be archived.

The production build embeds the small landing-page stylesheet in HTML through `vite.config.mjs`, removing a render-blocking request on mobile. Development retains Vite's CSS hot reload. If styles disappear after changing the build pipeline, inspect the emitted HTML for its `<style>` tag and verify the stylesheet asset lookup against Vite's output bundle; do not replace it with delayed style loading.

If TypeScript rejects an `Object.fromEntries` cast to the media library with TS2352, the generated object's keys are not proven complete. Keep the explicit typed four-clip record and shared clip factory in `src/media.ts`; do not mask it with a double cast. Run `npm run build` again after the correction.

In Windows PowerShell, use `npm.cmd run preview -- --port 4173 --strictPort` when forwarding flags. The `npm.ps1` shim can consume those flags and pass the port as a positional directory, causing a misleading missing-`dist` error even after a successful build.

If a DOM controller fails with TS18047 inside nested function declarations, an outer null check may not retain its narrowing in those functions. Bind fresh constants after checking the queried elements, so callbacks capture non-null types without assertions. Re-run the website build before browser verification.

If download fixtures reuse an earlier successful release during a fresh failure case on macOS, resolve the temporary directory with `realpath` before clearing Node's module cache. `/var` and `/private/var` can name the same directory; a literal prefix comparison otherwise misses cached modules. Clear only that fixture directory's entries and rerun `node scripts/test-download.mjs`.

If local preview becomes unresponsive while browser QA creates evidence, keep dependency discovery limited to index.html and exclude .local, media-src, dist, and .vercel from the development watcher. These private/generated trees can grow far beyond the application source and contain browser profiles and reports. Restart only the Team-bound website preview, then verify HTTP and actual image/video readiness; a failed DOM deadline alone does not establish a player defect.
