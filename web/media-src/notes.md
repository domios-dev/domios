# Domios landing media: edit and privacy log

Only sanitized exports live in `web/public/media/`. Raw recordings remain read-only on the owner's desktop. Contact sheets and inspection frames live in the private AgentPad and are not committed. Coordinates below are `(x, y, width, height)` in original source pixels, before cropping or scaling. Timestamp intervals are half-open and measured from each source recording.

## Source inspection

Inspected the short recordings with one frame per second, the long conversation with five-second contact sheets, and full-resolution frames around each retained action. Reviewed edited contact sheets and mask boundaries after encoding.

| Source | Duration | Timeline and edit decision |
| --- | ---: | --- |
| board | 7.424 s | 0-7.424: stable kanban board with occasional pointer movement. Five work states are visible. Keep 0.2-7.2; crop away navigation and the client plan header; blur task contents. |
| mesh | 21.696 s | 0-1.9: wide machine map. 1.9-2.9: zoom/pan toward a peer. 2.9-4.9: peer and Open button. 4.9-6.7: browser opens and loads. 6.7-11.7: remote chat including a private morning brief. 11.7-14: machine picker. 14-17.7: another machine chat. 17.7-21.696: browser closes and map pans. Keep a frozen initial map, the stable peer close-up, a stable sanitized browser moment, then return to the opening map. Cut loading, picker, unrelated chat and the final pan. |
| plugins | 19.029 s | 0-1.4: previous private chat while opening Plugins. 1.4-3.8: provider/plugin list. 3.8-6.1: Claude controls. 6.1-6.7: LM Studio settings with an endpoint. 6.7-8.7: Git information. 8.7-10.8: Bitwarden connection with a private endpoint/account state. 10.8-15: Chat Preprocessor documentation and scrolling. 15-19.029: idle and pointer movement. Cut the opening chat and LM Studio connection; mask Bitwarden fields; trim the ending. |
| mesh-talk | 147.029 s | 0-9.5: typing the request. 9.5-16: submitted request/loading and context insertion. 16-26: acknowledgement and tool setup. 26-29: temporary terminal view with session/path data. 29-42: preparation of a check-in. 42-66: dispatch and waiting. 66-80: mesh map with lit connections and moving message markers. 80-85: Activity log with private brief contents. 85-91.5: map. 91.5-108: reply cards, personal and project status, scrolling. 108-126: acknowledgement and waiting. 126-135: thanks and waiting. 135-147.029: final fleet summary. Retain request/setup, compress dispatch waiting, show map activity, use the stable 95-97.5 reply view with all reply bodies removed, then keep the safe final summary. |

## Edited timeline

All video uses 1600 x 1256, 24 fps, no audio. Aspect ratios are preserved with a dark matte where needed. Speed changes are hard cuts between deliberate demonstration beats. The mesh returns to exactly the opening sanitized frame; the board has a near-static loop. Plugins and mesh-talk use a chapter-reset cut when looped. No claim that every machine answers: four replies arrived, two peers were offline, and one did not answer before the check-in closed.

### board

| Edited seconds | Source seconds | Rate | Caption |
| --- | --- | --- | --- |
| 0-7 | 0.2-7.2 | 1.000x | One board for the whole team |

### mesh

| Edited seconds | Source seconds | Rate | Caption |
| --- | --- | --- | --- |
| 0-2 | 0-0.0333333 | Frozen first source frame | Your machines, connected |
| 2-6 | 2.95-4.75 | 0.450x | Open a peer |
| 6-10 | 6.7-7.4 | 0.175x | The same workspace in your browser |
| 10-12 | 0-0.0333333 | Frozen first source frame | One shared mesh |

### plugins

| Edited seconds | Source seconds | Rate | Caption |
| --- | --- | --- | --- |
| 0-3 | 1.4-5.6 | 1.400x | Choose your agent |
| 3-5 | 7.5-8.5 | 0.500x | Add capabilities |
| 5-7 | 8.75-10.65 | 0.950x | Connect your secret store |
| 7-10 | 11-14.9 | 1.300x | Extend your workspace |

### mesh-talk

| Edited seconds | Source seconds | Rate | Caption |
| --- | --- | --- | --- |
| 0-3 | 16-19 | 1.000x | Ask your agent about the mesh |
| 3-6 | 34-42 | 2.667x | Prepare one check-in |
| 6-9 | 46-64 | 6.000x | Contact the online machines |
| 9-16 | 68-80 | 1.714x | Watch messages cross the mesh |
| 16-21 | 95-97.5 | 0.500x | Replies arrive |
| 21-26 | 139-146 | 1.400x | One fleet summary |

## Redaction register

All retained scenes crop away the original workspace sidebar, client/project navigation and title-bar/desktop chrome. The board crop also removes the entire private plan header. The remote-browser crop removes its address bar, browser bookmarks and outer desktop navigation; its internal workspace navigation is masked. The entire discarded source footage is excluded from every video and poster.

Opaque masks replace pixels completely. The board uses `boxblur=22:3` across each column's private card text; only five explicitly reviewed first-row public state badges are restored. Client names, private task/plan titles, task descriptions and branch/handle metadata are consequently unreadable. Mesh masks remove all node IPs and service lists, including personal services and project endpoints. Host display names are retained as labels, not addresses.

Browser masks remove both private project output and the entire morning-brief/calendar body. Plugin masks remove the unreviewed local plugin entry and the Bitwarden connection/account panel. Mesh-talk masks remove username-bearing context and command paths, the personal name in outgoing check-in/acknowledgement text, and the entire incoming report bodies containing personal documents, project briefs, calendar/auth state and work paths. Only public tool headers and the safe fleet summary remain.

No token or email was knowingly retained. The redaction policy removes complete fields and report bodies rather than relying on spotting individual secrets.

### board: exact source regions

| Source seconds | Operation | Regions |
| --- | --- | --- |
| 0.2-7.2 | Keep only crop | `(350, 300, 1680, 1010)` |
| 0.2-7.2 | Blur private content | `(378, 365, 279, 957)`; `(711, 365, 281, 271)`; `(1044, 365, 287, 957)`; `(1381, 365, 283, 957)`; `(1715, 365, 281, 957)` |
| 0.2-7.2 | Restore public state badge | `(385, 456, 65, 26)`; `(719, 456, 85, 26)`; `(1054, 456, 80, 26)`; `(1389, 456, 69, 26)`; `(1723, 456, 49, 26)` |

### mesh: exact source regions

| Source seconds | Operation | Regions |
| --- | --- | --- |
| 0-0.0333333 | Keep only crop | `(350, 60, 1680, 1320)` |
| 0-0.0333333 | Opaque cover | `(1117, 183, 135, 35)`; `(906, 220, 342, 72)`; `(753, 358, 135, 35)`; `(542, 395, 342, 72)`; `(1483, 358, 135, 35)`; `(1272, 395, 342, 72)`; `(1117, 634, 135, 35)`; `(906, 671, 342, 103)`; `(663, 752, 135, 35)`; `(452, 789, 342, 72)`; `(1573, 727, 135, 35)`; `(1362, 764, 342, 122)`; `(915, 1064, 135, 35)`; `(704, 1101, 342, 76)`; `(1321, 1064, 135, 35)`; `(1110, 1101, 342, 76)` |
| 2.95-4.75 | Keep only crop | `(700, 340, 1040, 816)` |
| 2.95-4.75 | Opaque cover | `(1300, 505, 215, 80)`; `(790, 638, 670, 184)` |
| 6.7-7.4 | Keep only crop | `(495, 45, 1460, 1310)` |
| 6.7-7.4 | Opaque cover | `(495, 270, 334, 1050)`; `(835, 147, 1090, 519)`; `(835, 730, 1090, 455)`; `(835, 1185, 1090, 65)` |
| 0-0.0333333 | Keep only crop | `(350, 60, 1680, 1320)` |
| 0-0.0333333 | Opaque cover | `(1117, 183, 135, 35)`; `(906, 220, 342, 72)`; `(753, 358, 135, 35)`; `(542, 395, 342, 72)`; `(1483, 358, 135, 35)`; `(1272, 395, 342, 72)`; `(1117, 634, 135, 35)`; `(906, 671, 342, 103)`; `(663, 752, 135, 35)`; `(452, 789, 342, 72)`; `(1573, 727, 135, 35)`; `(1362, 764, 342, 122)`; `(915, 1064, 135, 35)`; `(704, 1101, 342, 76)`; `(1321, 1064, 135, 35)`; `(1110, 1101, 342, 76)` |

### plugins: exact source regions

| Source seconds | Operation | Regions |
| --- | --- | --- |
| 1.4-5.6 | Keep only crop | `(350, 60, 1680, 1320)` |
| 1.4-5.6 | Opaque cover | `(360, 1120, 540, 196)` |
| 7.5-8.5 | Keep only crop | `(350, 60, 1680, 1320)` |
| 7.5-8.5 | Opaque cover | `(360, 1120, 540, 196)` |
| 8.75-10.65 | Keep only crop | `(350, 60, 1680, 1320)` |
| 8.75-10.65 | Opaque cover | `(360, 1120, 540, 196)`; `(939, 310, 1070, 275)` |
| 11-14.9 | Keep only crop | `(350, 60, 1680, 1320)` |
| 11-14.9 | Opaque cover | `(360, 1120, 540, 196)` |

### mesh-talk: exact source regions

| Source seconds | Operation | Regions |
| --- | --- | --- |
| 16-19 | Keep only crop | `(350, 60, 1680, 1320)` |
| 16-19 | Opaque cover | `(510, 126, 1380, 52)`; `(678, 347, 1182, 66)` |
| 34-42 | Keep only crop | `(350, 60, 1680, 1320)` |
| 34-42 | Opaque cover | `(510, 126, 1380, 52)`; `(678, 347, 1182, 66)`; `(580, 675, 1275, 73)` |
| 46-64 | Keep only crop | `(350, 60, 1680, 1320)` |
| 46-64 | Opaque cover | `(510, 126, 1380, 52)`; `(678, 347, 1182, 66)`; `(580, 675, 1275, 73)` |
| 68-80 | Keep only crop | `(350, 60, 1680, 1320)` |
| 68-80 | Opaque cover | `(1239, 234, 135, 35)`; `(1028, 271, 342, 73)`; `(875, 408, 135, 35)`; `(664, 445, 342, 73)`; `(1603, 408, 135, 35)`; `(1392, 445, 342, 73)`; `(1239, 686, 135, 35)`; `(1028, 723, 342, 105)`; `(785, 806, 135, 35)`; `(574, 843, 342, 73)`; `(1695, 779, 135, 35)`; `(1484, 816, 342, 126)`; `(1038, 1118, 135, 35)`; `(827, 1155, 342, 79)`; `(1442, 1118, 135, 35)`; `(1231, 1155, 342, 79)` |
| 95-97.5 | Keep only crop | `(350, 60, 1680, 1320)` |
| 95-97.5 | Opaque cover | `(587, 395, 1265, 65)`; `(515, 868, 1350, 310)`; `(515, 1243, 1350, 15)` |
| 139-146 | Keep only crop | `(350, 60, 1680, 1320)` |
| 139-146 | Opaque cover | `(515, 110, 1350, 350)`; `(586, 650, 1265, 40)` |

## Reproduce and consume

Run from any checkout containing this script:

```powershell
python web/media-src/build-media.py --source-dir "C:/Users/dev/Desktop"
```

FFmpeg and ffprobe must be on PATH. The build script uses only the Python standard library. It records every crop, cover, blur, restored badge, trim, freeze, speed factor and caption in one place. Redacted intermediate H.264 files live in an automatically removed system temporary directory; originals are never edited or copied into the repo.

Per clip: `/media/<id>.mp4` (H.264, CRF 26, yuv420p, faststart), `/media/<id>.webm` (VP9, CRF 36), `/media/<id>-poster.webp` (quality 82, taken from the sanitized MP4). IDs: `board`, `mesh`, `plugins`, `mesh-talk`. Postprocessing removes source metadata and every audio stream.

`captions.json` is an object keyed by clip ID, with arrays of `{start,end,text}` in edited seconds. `manifest.json` has `{schemaVersion,totalBytes,clips}`; each clip has `{id,duration,width,height,loop,mp4,webm,poster,captions}`. File entries have `{src,bytes}`; `src` is an absolute site path. `totalBytes` includes the twelve media assets and both JSON files.

## Verification and review boundary

The pipeline checks durations, dimensions, pixel format, one video stream/no audio, the aggregate 15 MB budget, and a complete FFmpeg decode of both encodes for every clip. Final manual frame review covers retained beats, transitions and posters. No application build, Cargo command, project gate, dev server or Playwright run is involved.

Original app labels still say CodeTerm; this is recorded product UI, while the landing product is Domios. Device display names and the public fleet health/upgrade summary remain visible. No unremoved client or personal brief is intended to remain. The owner may prefer neutral device names or a cleaner staged Board recording: the conservative board redaction necessarily obscures its task text.

## Final export inventory

| Clip | Seconds | MP4 bytes | WebM bytes | Poster bytes |
| --- | ---: | ---: | ---: | ---: |
| board | 7 | 67,108 | 43,653 | 17,886 |
| mesh | 12 | 145,450 | 138,962 | 20,882 |
| plugins | 10 | 586,376 | 438,344 | 51,530 |
| mesh-talk | 26 | 281,885 | 345,761 | 21,534 |

Total including JSON: **2,164,309 bytes** (2.164 MB).

All eight video files passed complete decode with FFmpeg `-xerror`, duration/dimension/pixel-format/no-audio checks. Additional independent inspection verified H.264/VP9/WebP codecs, exact manifest file sizes, contiguous caption spans through each clip duration, fourteen output files and MP4 `moov` atoms preceding `mdat`. Final edited frames were reviewed at 2 fps, with full-resolution review of sensitive boundaries and the last retained browser/reply frames. Scroll transitions that moved private text outside fixed masks were cut from the export, rather than relying on a mask during a moving viewport.
