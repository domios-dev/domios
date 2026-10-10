# Selected Domios trailer sources

The owner selected this 54.333-second film on 2026-10-03. It contains 1,630 frames at 30 fps, 14 spoken phrases, the additional Memory explanation, continuous retained-source music, and the computer finale. The complete project is stored in ordinary Git files, without Git LFS or a dependency on a runtime session folder. This directory is source material; it is never a public website asset directory.

| Directory | Contents |
| --- | --- |
| `recordings/` | All four original screen recordings and the native Memory recording |
| `voice-original/`, `voice-memory/` | Original Chris take, added Memory take, actual service character alignments and source receipts |
| `music-original/` | Original extracted Grok audio, the 45-second arrangement and its edit proof |
| `prepared/` | Extended voice and music WAVs, 14-cue timeline, alignment, edit lineage and computer finale still |
| `brand/` | All 49 exported authoritative Threshold poses and their source hash |
| `baseline-picture/` | Preserved 45-second picture used for the unchanged opening prefix |
| `picture/` | Complete selected 54.333-second picture without sound |
| `stems/` | Separate voice/music stems, the shared mix WAV and reconstruction/balance receipts |
| `selected/` | Exact owner-selected MP4/WebM package, captions, chapters, posters and credits |
| `receipts/`, `lineage/` | Original validation receipts and referenced historical source files |

`inventory.json` records the SHA256 and byte count of every archived input, plus its original location. Original receipts are preserved byte-for-byte. Their old absolute path strings are historical provenance, not rebuild dependencies; use the inventory to find the corresponding Git-owned file. One unavailable historical location is a directory string, not a missing required input. No credentials or API keys are required to rebuild. Voice and music are already recorded; rebuilding makes no generation request.

## Verify and rebuild

Run from `web/` with Python 3.12, Pillow, FFmpeg and FFprobe installed. The source pipeline is committed alongside this project. The accepted pipeline revision is `fde01c873ba30c3b3261c80b5241fbc0c6e7c5e2`; the archive commit adds the portable wrapper. Git preserves both revisions.

```powershell
python media-src/rebuild-archived-trailer.py --verify-only
python media-src/rebuild-archived-trailer.py --verify-only --git-ref HEAD
python media-src/rebuild-archived-trailer.py --output .local/rebuilt-memory-1
```

The second command verifies every archived file, reuses the preserved silent picture, and mixes/muxes the archived assembled voice and music with the existing pipeline. AAC uses the qualified fast coder; Opus uses the qualified 10 ms frame duration. Output must be a fresh directory. A rebuilt encode is a new candidate: exact byte identity across FFmpeg versions is not promised. The exact selected encode remains in `selected/` for comparison. Listen to and validate changed outputs before publication.

To rerender the inserted Memory episode and the computer finale from their source inputs:

```powershell
python media-src/rebuild-archived-trailer.py --render-picture --font C:/Windows/Fonts/segoeui.ttf --output .local/rerendered-memory-1
```

Segoe UI is an installed system font and is not redistributed here. Another installed font can be supplied on other platforms, with a corresponding visual review. The preserved picture needs no font. Rerendering consumes the preserved baseline picture prefix, native Memory footage, owner Mesh image in `media-src/assets/`, and brand poses. It refuses stale brand source hashes. The four native recordings remain available for deeper scene edits with `build-media.py` and `trailer_scenes.py`.

The archive is immutable during rebuild. Only the new output directory receives renders, mixes and proofs. There is no arbitrary file-size ceiling, no app build/install, no website deployment and no Lighthouse operation in this workflow.
