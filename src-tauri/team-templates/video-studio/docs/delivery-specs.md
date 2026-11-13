# Delivery specs

What a launch owner needs from the video studio. The `formats`, `voice`, `plays_on` and `deadline` start answers override these defaults.

## Exports

| Export | Frame | Length | Notes |
|---|---|---|---|
| Hero or trailer | 16:9, 1920x1080 | 45-90 s | Product Hunt takes a YouTube or Loom link, not a file |
| Short cut | 9:16, 1080x1920 | 15-30 s | X, LinkedIn, Reels, Shorts |
| Square cut (optional) | 1:1, 1080x1080 | 15-30 s | Feeds |
| Silent loop or GIF | any of the above | 3-8 s | Gallery slots, README, site hero; under 3 MB for a Product Hunt thumbnail GIF, under 10 MB elsewhere |
| Poster frame | same as its video | one frame | The first frame matters |

## Encoding

- MP4, H.264 video and AAC audio, `yuv420p`, `+faststart`.
- Every video ships as a full-quality master plus a share export under 25 MB. Email and messengers reject larger files; review stalls when a reviewer cannot receive the cut. Prefer a link to a file for review.

## Captions

- Burn captions into the share exports: autoplay is muted.
- Ship an `.srt` beside every captioned export.
- The first three seconds say what the product is, readable without sound.

## Manifest

Every delivery carries a source manifest: the scenes in order, the copy each one shows, and which app build each scene was captured from.

## Timing

- Final cut three days before the deadline; upload one day before.
- A UI or messaging change after capture means re-capturing the affected scenes; lock messaging before recording.
- The deadline lives in one place (the Team's goal). When it moves, search plans, drafts and listings for the old date.
