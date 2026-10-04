# Domios trailer review pipeline

The preserved silent original is 45 seconds at 24 fps. Its complete media directory and archive branch at `222f60c46b0f962d566344c99b96b5da3afade7f` remain read-only. These source tools prepare an independent candidate; they do not replace public media. Alvar owns all executable preparation, rendering, mixing, validation, listening, browser proof, selection and deployment. No worker gate or render has been executed.

The owner's later voice-led direction supersedes the frozen chapter and caption times. Keep four chapter IDs/order (`agents`, `board`, `mesh`, `plugins`) and thirteen cue IDs/order. Candidate duration is 40–55 seconds, targeting a natural 45 seconds, with native 30 fps picture. Every frame count derives from chosen duration. Original times are source defaults for independent silent preparation only.

## One timing authority

`prepare-timing.py` reads the actual complete take, its original service character alignment and the root's source metadata. It verifies the audio hash, walks all thirteen exact phrases and derives every word's actual source and final timestamps. The first candidate keeps the entire recording intact at its original speed with 0.65 seconds of initial silence. The recorded final phrase runs 36.652–42.028 seconds; its shifted end is 42.678 seconds, leaving a complete final hold before 45. No thirteen paid requests, word cropping, or speech segmentation are used.

The resulting private `timing.json` owns cue text/onsets/ends, visual cuts in real breath gaps snapped to native frames, chapter starts at visual cues 1/3/5/9, poster times, duration and per-word provenance. Builder, audit, mux and checker consume that same file; the render manifest pins its SHA256. Changing it requires a new picture candidate. Caption chips, seekable transcript and hidden VTT captions use the same media clock and exact generated words.

Root source metadata reports stock Chris / `eleven_multilingual_v2`, two paid requests and 644 total input characters including the sample. The full take lasts 42.057143 seconds. These are inspected root metadata facts, not new worker API calls or listening results.

## Source and camera decisions

Both supplied raw contact sheets were visually inspected before excerpt selection: `send-work-32-64.png` is 4×8, first frame 32 seconds, one frame per second; `reports-94-98.png` is 2×4, first frame 94 seconds, two frames per second. Coordinates below refer to those unmodified sheets.

| Cue | Real footage and purpose | Camera/motion |
| --- | --- | --- |
| 1 | mesh-talk frame 16, desktop establishing composition | Restrained analytic spring launch; decorative five-color dock removed; window fills the exact next recorded crop before the cut |
| 2 | mesh-talk 16 onward, up to four source seconds | Native-speed agent tabs; establish then settle into conversation |
| 3–4 | board 0.2–7.2, one source clock across both cues | Shared inertial trajectory, slower center, faster arrival/exit; equal position and nonzero first derivative at cue join |
| 5 | mesh frame 0, existing illustrated machine context | Duration-aware pull apart; packet size approaches zero at route endpoints before modulo wrap |
| 6 | mesh 0 onward, up to four source seconds | Recorded peer action, declared documentary cut after establishing shot |
| 7 | mesh-talk 33–35 and 46–48, shortened to fit visual window | Sheet row 1 columns 2–4 shows setup/tool result; row 4 columns 3–4 shows dispatch and background-terminal status. Two explicit native-speed excerpts omit waiting; no accelerated 32-second cursor sweep |
| 8 | mesh-talk 95–98 | Report sheet row 2 onward shows settled returned reports after the initial scroll. Fast arrival followed by readable hold; bounded crop `(460,300,1470)` to `(490,550,1390)` |
| 9–11 | plugins 1.5–5.6, 7.3–8.7, 11–14.9 | Providers, Git, documentation; intentional initial read hold, then controlled move; existing sensitive-account exclusions retained |
| 12 | mesh-talk 139–146 | Fleet payoff remains documentary; bounded crop `(460,570,1450)` to `(490,620,1350)` |
| 13 | Existing original03 Threshold outline and Domios close | Uniform static mark, no alternative/deformed geometry; complete last phrase and final hold; duration-aware return to the opening plate |

The default cue-7 cut skips 35–46 (eleven seconds); phrase-led shorter excerpts can skip slightly more. The cut is declared, never disguised as a continuous action. A/B output still must establish that each choice improves readability.

The original per-shot source ramp changes source speed by about 1.916 at output 11 seconds while the contiguous board footage crosses source 3.2. Its two independently eased camera moves stop at the join. Original report target y=740 exceeds its feasible 614.125; fleet y=850/900 exceeds 580.375/636.625, and runtime clamping changes the trajectory. The new paths avoid those authored bounds and clamping. These causal source facts do not prove that a visible jerk occurred: Alvar must tag actual original/candidate clips as confirmed, rejected or repaired. Native 30 fps is source cadence, not a repair claim.

Decoder reads exact native frames from the top-left 2048×1396 source crop. Audit records continuous source time, the chosen native source frame/time and camera box for every output frame, analytic board join position/velocity and all declared hard cuts. No interpolation, mask, blur, fabricated response or extra source write occurs. Actual last/first decoded output frames determine loop quality; differing invisible end-card source frames are not causal evidence.

## Canonical commands

Run from canonical `web/`, serially in a Team utility shell, under Alvar's bounded runner. Set `$trailerReview` to Alvar's absolute private review path and `$trailerAudio` to `D:/Developer/codeterm/web/.local/trailer-audio-source`. All output directories must be empty. Preserve failed output evidence; choose a fresh output directory for a reviewed correction rather than overwrite it. Probe timeout 120 seconds, full visual command 1800 seconds, mix/mux 300 seconds; timeout fails.

```powershell
python media-src/prepare-timing.py --source-metadata "$trailerAudio/narration-source.json" --alignment "$trailerAudio/alignment.json" --audio "$trailerAudio/narration-full.mp3" --output "$trailerReview/timing.json" --offset 0.65 --duration 45
python media-src/check-media.py --timeline-only --timing "$trailerReview/timing.json" --baseline-ref 222f60c46 --audit "$trailerReview/timeline.json"
node media-src/export-threshold.mjs --output-dir "$trailerReview/brand" --fps 30 --duration 1.6
python media-src/build-media.py --source-dir C:/Users/User/Desktop --font C:/Windows/Fonts/segoeui.ttf --timing "$trailerReview/timing.json" --brand-frames "$trailerReview/brand" --output-dir "$trailerReview/probe-board" --review-range 10:12
python media-src/build-media.py --source-dir C:/Users/User/Desktop --font C:/Windows/Fonts/segoeui.ttf --timing "$trailerReview/timing.json" --brand-frames "$trailerReview/brand" --output-dir "$trailerReview/silent" --preview-dir "$trailerReview/silent-proof"
python media-src/check-media.py --baseline-dir .local/trailer-before-222f60c46/media --baseline-manifest .local/trailer-before-222f60c46/sha256.json --candidate-dir "$trailerReview/silent" --timing "$trailerReview/timing.json" --audit "$trailerReview/silent-audit.json"
python media-src/mux-narration.py --video-dir "$trailerReview/silent" --audio "$trailerAudio/narration-full.mp3" --alignment "$trailerAudio/alignment.json" --timing "$trailerReview/timing.json" --music "$trailerAudio/chill-wave.mp3" --music-metadata "$trailerAudio/music-license.json" --output-dir "$trailerReview/narrated" --proof-dir "$trailerReview/sound-proof"
python media-src/check-media.py --baseline-dir .local/trailer-before-222f60c46/media --baseline-manifest .local/trailer-before-222f60c46/sha256.json --candidate-dir "$trailerReview/narrated" --reference-video-dir "$trailerReview/silent" --timing "$trailerReview/timing.json" --narration-alignment "$trailerReview/sound-proof/narration-alignment.json" --require-audio --audit "$trailerReview/final-audit.json"
```

Repeat private review exports for `3:5`, `18:20`, `23:30`, `38:41`, `44:45` and `0:1`, passing `--brand-frames "$trailerReview/brand"` to each build command. Also derive short frame-snapped ranges around every actual `visualStart` in timing.json, including the new board join and cue-7 internal cut: original times alone no longer identify the phrase-led joins. `--review-range` emits MP4/WebM and audit only, with no publishable manifest. Compare original/candidate by timestamps and equivalent story beats, accounting for the original 24 fps clock, not equal frame index. Keep full A/B, source-time audits, boundary frame pairs, a native-cadence whole-candidate viewing, storyboard and owner selection.

## Music, mux and audio proof

`--music` and `--music-metadata` are paired optional inputs. Root supplied Chill Wave by Kevin MacLeod with CC BY 4.0 evidence. Root remains responsible for license verification and taste/listening. Its credit appears visibly in the existing transcript disclosure, in both container metadata and accompanying public credits files. All accompanying files count toward the 6,000,000-byte aggregate.

The mux keeps the continuous speech untouched in time, adds initial silence and pads only final silence to the chosen duration. Voice mastering targets -18 LUFS; music bed targets -30 LUFS with speech-keyed ducking, a one-second entrance and intentional three-second ending. Two-pass programme mastering targets -18 LUFS and -1.5 dBTP to leave codec headroom; actual encoded output must pass -18±1 LUFS and ≤-1 dBTP. Root must judge music level, clicks, breath continuity, pronunciation and every cue in complete A/V. No audible playback is launched by these tools.

H.264/VP9 video streams are copied during mux; audio is mono 48 kHz AAC/Opus. Private sound proof keeps original-source/alignment hashes, aligned voice WAV, final mixed WAV hash and per-word source-to-final mapping. Full decode, frame hashes, timing authority, actual original character timestamps and all published assets are checked by `check-media.py`. The oracle also re-encodes the proof WAV with the same audio settings and compares compressed audio packet SHA256 with the actual candidate stream. Voice and music metadata alone do not prove the audible result.

WebM PTS may quantize to the probed millisecond stream time base. Every frame must remain within one actual tick of index/30 relative to firstPTS, be strictly increasing, have declared CFR30 and the exact chosen frame count. MP4 retains sub-1e-5 residual where its measured timescale supports it. Audit includes firstPTS, measured time base, maximum residual, tolerance and count. Quantization cannot excuse frame duplication, omission or cadence drift.

## Post-publication framing and opening audio

The fresh framing pass keeps the published 45s/30fps clock, all thirteen cues and all four chapters. `export-threshold.mjs` reads `src/brand/motion.ts` without editing it, records that source SHA, and rasterizes its 49 exact poses through a private headless Edge session. Frame 48 is the exact REST pose. `build-media.py` consumes those frames and changes only `[0,48)` and `[1112,1350)`. The blue mark sits within the existing desktop composition and uses an eight-frame cosine alpha handoff at the opening boundary without changing a pose or path. The finale uses the real plugin, fleet and machine recordings, the static original mark, factual copy, and `domios.dev`. The current Showcase puts transport controls and caption chips inside the video picture; canonical full-player review must check the mobile finale logo and URL against these overlays. The existing cut and source camera path outside those ranges remain fixed.

The bounded audio mode consumes the previous complete narration proof and its actual mastered stems. It checks arranged WAV bytes against `credit.sha256` and independently checks the extracted opening source against `credit.extractedAudioSha256`. A short cosine ramp starts the excerpt at output zero; a bounded cosine join returns to the approved arrangement at four seconds. The full mastered voice stem and prior final PCM16 samples after the four-second boundary are copied byte-for-byte, and later mastered music samples are preserved. Only the first four seconds of music and final PCM can change. The gain is bounded by the actual opening onset floors and the existing 14dB phrase-separation rule. Existing mastering is reused without global normalization. Visible credit stays identical to the approved source attribution; excerpt timing, gains, ranges and hashes live in dedicated manifest/proof lineage. A candidate that cannot meet the unchanged loudness, true-peak, phrase, or size oracles is a reported limitation; voice and later music are not adjusted.

Run from canonical `D:/Developer/codeterm/web` after the ancestry-preserving merge. Root creates a fresh empty private `$review`, verifies the supplied proof paths/hashes, and snapshots `public/media` before any command. `$baseline` is the immutable snapshot with `media/` and sibling `sha256.json`; `$timing`, `$audio`, `$oldProof`, `$music`, and `$credit` are the root-owned inputs listed in task DW-11. No command below writes to `public/media`.

```powershell
node media-src/export-threshold.mjs --output-dir "$review/brand" --fps 30 --duration 1.6
python media-src/build-media.py --source-dir C:/Users/User/Desktop --font C:/Windows/Fonts/segoeui.ttf --timing "$timing" --brand-frames "$review/brand" --review-range 0:3 --output-dir "$review/intro" --preview-dir "$review/intro-proof"
python media-src/build-media.py --source-dir C:/Users/User/Desktop --font C:/Windows/Fonts/segoeui.ttf --timing "$timing" --brand-frames "$review/brand" --review-range 37.0666666667:45 --output-dir "$review/end" --preview-dir "$review/end-proof"
python media-src/build-media.py --source-dir C:/Users/User/Desktop --font C:/Windows/Fonts/segoeui.ttf --timing "$timing" --brand-frames "$review/brand" --output-dir "$review/silent" --preview-dir "$review/silent-proof"
python media-src/mux-narration.py --video-dir "$review/silent" --audio "$audio/narration-full.mp3" --alignment "$audio/alignment.json" --timing "$timing" --music "C:/Users/User/.codeterm/runtime/agent-pads/2652c9dc/grok-full-preview-2/grok-arranged.wav" --music-metadata "$credit" --opening-source "$music" --baseline-audio-proof "$oldProof" --opening-source-start 2.6 --opening-end 4 --output-dir "$review/narrated" --proof-dir "$review/sound-proof"
python media-src/check-framing.py --baseline-ref 072a5b911 --source-dir C:/Users/User/Desktop --font C:/Windows/Fonts/segoeui.ttf --timing "$timing" --brand-frames "$review/brand" --baseline-audio-proof "$oldProof" --candidate-audio-proof "$review/sound-proof/narration-alignment.json" --audit "$review/framing.json"
python media-src/check-media.py --baseline-dir "$baseline/media" --baseline-manifest "$baseline/sha256.json" --baseline-ref 222f60c46 --candidate-dir "$review/narrated" --reference-video-dir "$review/silent" --timing "$timing" --narration-alignment "$review/sound-proof/narration-alignment.json" --require-audio --audit "$review/media.json"
python -m unittest discover -s media-src -p test_framing.py
```

The opening/end renders are technical probes. Inspect their exact boundary frames before the complete render; then inspect and listen to the complete candidate and compare screenshots/decoded residuals. `check-framing.py` proves raw renderer equality on every untouched frame, compares all exact Threshold poses, measures decoded encoded-frame residuals without calling them equality, confirms the exact voice and post-four-second music PCM, and records the source hashes, gain, ramp, crossfade and changed samples. `check-media.py` remains the unchanged default publication oracle, including duration, words, chapters, balance, programme loudness, true peak, PTS, full decode, encoded audio, file inventory and accurate byte counts. Root owns listening, full-picture review and replacement.

Required tools and inputs: Node 22 with the installed TypeScript package, Microsoft Edge (or `DOMIOS_EDGE_PATH`), Python/Pillow, FFmpeg/FFprobe, Segoe UI, the four existing desktop recordings, root timing/alignment/full narration, prior audio proof and its actual mastered stems, arranged WAV, approved extracted-source audio and credit metadata. The source lane did not render or run these commands. No measured onset, encoded residual, programme loudness, true peak, final package size or listening result is claimed here.

If size or legibility fails, report the result for a reviewed encoding/copy choice; do not weaken gates or silently clip speech. No private masters, raw audio or audit files enter public/media.

## Player proof still required

<!-- revision: 1 -->

One video element owns picture, speech and music. Fresh automatic playback is muted. Sound opt-in is explicit and keyboard accessible; opted-in video stops at the end, while muted playback can loop. Offscreen/hidden transitions pause immediately and remute, so automatic return stays silent. Explicit replay restarts after the end. Rejected play promises leave honest Play state. Reduced motion makes no video/track requests before an explicit Play/chapter/sound gesture. VTT tracks stay hidden, avoiding duplicate captions/announcements; visible caption chips and transcript use the actual final words.

Alvar runs `node node_modules/typescript/bin/tsc --noEmit`, then the existing canonical build/preview/CDP/Lighthouse contract. Verify both themes at 1440×1100 and 360×900, normal/reduced motion, pointer/Enter/Space sound toggles, seek/chapter/focus behavior, captions at actual audio timestamps, muted reload/automatic resume, narrated end/replay, visibility/offscreen pause, rejected play promises, no toolbar collision/overflow, visible attribution and silent manifests with no sound toggle. No Playwright. Preserve all six existing website proofs and the desktop100/mobile≥99 bar.
