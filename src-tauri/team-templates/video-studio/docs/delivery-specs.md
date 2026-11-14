# Delivery specs

What a launch owner needs from the video studio. These are suggested defaults: the Director agrees where the film plays, the formats, voice and captions, and the deadline with the owner in the first conversation, and the Team's goal carries the answers.

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
- The master also carries the captions as a soft subtitle track (`mov_text` in MP4), so players and platforms can switch them, and container tags for title, creator, year and copyright. Share exports keep burned-in captions.

## Manifest

Every delivery carries a source manifest: the scenes in order, the copy each one shows, and which app build each scene was captured from. Generated material (docs/local-generation.md) has its own `generated` row: the slot, the provenance file and the model's licence.

## After upload

A platform reprocesses what it receives. Watch the published video on the platform itself: the resolution after processing, the captions, the thumbnail and the first three seconds. High resolutions can appear later than the upload; report that rather than calling it done.

Embedding subtitles and media tags follows the practice of Tyrrrz/YoutubeDownloader (MIT); the wording is ours.

## Timing

- Final cut three days before the deadline; upload one day before.
- A UI or messaging change after capture means re-capturing the affected scenes; lock messaging before recording.
- The deadline lives in one place (the Team's goal). When it moves, search plans, drafts and listings for the old date.
