# Domios trailer

The landing page plays one 45-second, 1280 × 720, 24 fps trailer. Four shelf buttons seek within that same video: agents at 0:00, team board at 0:08, connected machines at 0:15, and tools at 0:30. Playback drives the selected chapter and HTML narration chips. The player does not load video sources or autoplay when reduced motion is requested; an explicit play or chapter action enables playback.

## Reproduce

Requires Python 3.12+, Pillow 10+, FFmpeg 7.1+ and ffprobe on PATH. Raw recordings stay read-only on the owner's desktop and are never copied into the repository.

```powershell
python web/media-src/build-media.py --source-dir 'C:/Users/dev/Desktop' --preview-dir '<your-agent-pad>'
```

`--font` accepts a TrueType font path. The shipped render uses Segoe UI. `--output-dir` allows a review export outside the published assets. The pipeline renders an intermediate H.264 master in an automatically removed temporary directory, then H.264 MP4 first, VP9 WebM second, one main WebP poster, and four small chapter stills. Old individual clip encodes are removed from the chosen output directory by exact filename.

## Edit map

Source ranges are seconds in the original recordings. Camera tuples in `trailer_scenes.py` are `(left, top, width)`; the camera preserves a 16:9 viewport. Quintic easing guides each camera move. Source-time remapping accelerates through the start of a shot and slows into its payoff. No generated card transitions or invented agent responses are inserted into real footage.

| Trailer | Source | Source range | Story |
| --- | --- | --- | --- |
| 0–4 | mesh-talk | 16–16.1 | Rendered desktop, wallpaper, dock, and Domios window opens; camera approaches the actual recorded app |
| 4–8 | mesh-talk | 16–23 | Named agents and a conversation in the app |
| 8–11 | board | 0.2–3.2 | Wide board pushes into the first task columns |
| 11–15 | board | 3.2–7.2 | Camera travels across review, blocked, and done; the source board itself is mostly static |
| 15–19 | mesh | 0–0.1 | Rendered laptop and two displays pull apart, connected by moving droplets |
| 19–23 | mesh | 0–4.8 | Real map moves toward a peer and its Open action |
| 23–27 | mesh-talk | 32–64 | Waiting is compressed into agent setup and dispatch |
| 27–30 | mesh-talk | 94–98 | Camera pushes into returned reports |
| 30–33 | plugins | 1.5–5.6 | Provider choices and controls |
| 33–35.5 | plugins | 7.3–8.7 | Git capability |
| 35.5–38 | plugins | 11–14.9 | Plugin documentation and scrolling |
| 38–41 | mesh-talk | 139–146 | Fleet summary, including offline peers |
| 41–45 | rendered | — | Threshold mark settles, domios.dev, then the opening desktop for the loop |

The four recordings differ slightly in pixel dimensions. Decode normalizes them with a top-left 2048 × 1396 crop before sending raw frames through the renderer. This avoids raw pipe frame-size mismatches while keeping text undistorted. The pipeline fails on missing or incomplete source frames.

## Visual treatment

All device/window composition, wallpaper, eased camera moves, link droplets, title beats, and the Threshold end card are reproducible Python/Pillow rendering. The Threshold geometry is embedded in the renderer; no build-time dependency on another worktree exists. Short title beats provide story signposts. Narration remains separately timed HTML via `captions.json` and an accessible, seekable transcript.

The owner explicitly requested clear, unmasked footage: there are no blur or redaction filters. Retained app content is visible. Original UI still says CodeTerm in places; the rendered window chrome and end card use Domios. Sources were inspected through contact sheets and full-size frames, with review previews kept in the private AgentPad. The plugins edit excludes the Bitwarden account and local endpoint views.

## Assets and verification

`manifest.json` schema 2 describes the single trailer, chapter boundaries, dimensions, duration, asset URLs, byte counts and exact aggregate size. `captions.json` is an ordered array of `{start,end,text}` on the single trailer timeline. The main poster is the connected-computers payoff at 18 seconds; chapter stills are 480 pixels wide.

Each build verifies one video stream, no audio, 1280 × 720, yuv420p, 45 seconds, a complete decode of both formats, and the aggregate media budget below 6,000,000 bytes. MP4 uses faststart, H.264 CRF 27; WebM uses VP9 CRF 39. Posters use WebP quality 84. Preview mode exports a 640-pixel GIF and six full-size story frames with a contact sheet.

Final media is 5,800,809 bytes: H.264 2,884,727 bytes, VP9 2,846,694 bytes, plus posters and JSON. A browser downloads its supported format, not both. The initial page does not fetch either video until the player becomes visible.

Layout and browser behavior are reviewed at 1440, 1280, 1024, 768 and 360 pixels in both themes using headless Edge/CDP. Desktop shelf/player edges align; mobile keeps the 2×2 shelf above the video. Review includes reduced-motion loading, explicit chapter playback, automatic chapter/caption following, keyboard navigation, and the return to chapter one at the loop boundary. Production verification uses `npm run build` and desktop/mobile Lighthouse. No Playwright or application build is involved.

## Troubleshooting

- A raw-frame render that stalls at a shot boundary means decoder dimensions and the pipe reader disagree. Keep the normalization crop before rawvideo output, and keep `SOURCE_SIZE` identical to that crop; do not resize source recordings on disk.
- An aggregate-byte assertion off by the JSON line count means Windows newline translation changed the manifest after measurement. The writer explicitly uses LF; calculate bytes from the same serialized UTF-8 text.
