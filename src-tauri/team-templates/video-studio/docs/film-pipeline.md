# The HTML film pipeline

How a product film is made from the real app: what was actually done for a 3-minute film, a 70-second trailer and a live web film, and what it cost. Adapt the names to the project; keep the rules.

## The idea

A scene is a web page. It is built from the product's real UI components fed with fictional fixture data, never from mock-ups or screen recordings. Time is an input: every scene renders a pure function of time, so any frame can be reproduced, reviewed and captured exactly.

## Order of work

1. **Script.** Narration text and one message per beat, iterated with the owner until approved. Lock the messaging before anything is recorded: messaging that changes after assets exist means rework.
2. **Voice and music.** Only for a voiceover: text-to-speech with word timestamps gives the timing for captions and cuts. Music sits on its own track. Captions-only films take their caption timing from the script beats.
3. **Representative sample.** Render one scene to stills and a short clip before building the rest, so style, motion and tone are judged on real output early.
4. **Scenes.** One file per scene (for example `scenes/<group>/<id>.scene.tsx`) exporting its id, its duration and `render(t)`. A scene imports the real components; fixture data lives in its own folder.
5. **Reels.** A reel composes scenes on one timeline (narration-timed segments, trims, holds, fits). The reel publishes its cut list for the tooling, and one contract table pins every scene's duration. One editor owns the reels.
6. **Iterate on stills.** A playground page scrubs to any time; the owner reviews stills, not a half-built live page.
7. **Render.** Frame-exact capture to video, mux voice and music, then the captioned variant and the `.srt`.
8. **Web embed** (only when it plays on a site): build the film as one module with its CSS isolated (a shadow DOM), add fallback clips for weak devices, integrate by pull request, preview, owner OK, merge, then verify the deployed page.

## Rendering

- Headless Chromium through Playwright. Install a paused page clock and seek the film frame by frame (`seek(t)`); capture in a few parallel pages, then encode with ffmpeg. Never use screen recording.
- Fail the capture if any frame shows a missing-asset marker; a blank frame is a defect, not a pause.
- WebGL scenes need a GPU-enabled headless browser; load heavy 3D only as a lazy chunk.
- Defaults: 1920x1080 at 30 fps, device pixel ratio 1, four capture pages, H.264 MP4 plus `.srt`.
- Freeze the palette and the fixes, then render. Long GPU captures made while editing continued were thrown away.
- Run long captures and encodes in a utility shell with a timeout, not inside an agent's own turn, and close the shell when its result arrives.
- Generated backgrounds or b-roll are optional and come only from a local tool (docs/local-generation.md). They never replace a product scene.

## Review tools

- Stills at key times, in dark and light themes and in landscape and portrait.
- A contact sheet per render: one time-stamped frame at every cut plus one per second, to judge cuts and pacing at a glance.
- Probes that play the real page in real time and under phone emulation with 4x CPU throttling: page errors, blank frames, memory, frame times, Lighthouse.

## Files

- Film code is versioned next to the app, because scenes import real components: engine and player, `scenes/`, `reels/` with the registry and duration contract, `fixtures/`, `assets/`, `embed/`.
- Capture and probe scripts live with the code or in one tooling folder, not scattered in temp folders.
- Media stays out of git in one media folder: `renders/` for finals, `sources/` for voice, caption timing and bundles, `archive/` for old iterations, `audio/`, `stills/`. Move files, never copy them; never delete without the owner. A README there says who owns what.

## Keeping a long film on track

A film runs for days and many owner rounds; agents are replaced, resumed and compacted along the way. One short state file keeps it on track.

- **`film-state.md`** in the media folder, owned by the Director: the approved script version, the approved sample, the frozen palette and decisions, the current render list, and the round log. Every Worker reads it first and works from it, not from memory.
- **Round log:** each owner round is numbered and keeps the owner's notes verbatim, with the answer to each note (done, changed, declined with the reason). Summaries lose the small notes that cost the most rounds.
- **Large output stays in files:** probe output, contact sheets, frame dumps and logs are written to files and referenced by path. Briefs and reports carry the path and a one-line result, never the dump.

Sources: the index-first and verbatim-memory ideas come from thedotmack/claude-mem (Apache-2.0) and MemPalace/mempalace (MIT); keeping large output out of context, from alexgreensh/token-optimizer; long commands as bounded background sessions, from wonderwhy-er/DesktopCommanderMCP (MIT). The wording and rules here are ours.

## What cost the most

- Wall-clock time. Anything reading the clock or running effects later had to be settled before every screenshot. Make time deterministic from day one.
- Phones. WebGL contexts leaked per cut (release them when a cut ends), canvas backing at DPR 3 reached 17-24 megapixels (cap it near 8-10), and some crashes showed only on a real device. Profile on phone emulation early, not after launch.
- Blank frames at cut boundaries from lazy loading. Never show an empty frame: preload every cut at mount and hold the last drawn frame.
- The light theme. Additive glow vanishes on white; every colour needs a light variant from the start.
- Showing the owner a hot-reloading page mid-edit. They saw a half-built state and thought the feature was gone. Show stable builds or previews only.
- Bundler and package manager details: a library build may not minify whitespace unless told to, and the project's own package manager must be used.
- Many small owner notes per frame are normal. Budget five to ten rounds for each key scene.
