#!/usr/bin/env python3
"""Build redacted landing-page media with FFmpeg 7.1+ and Python 3.12+."""

import argparse
import json
import subprocess
import tempfile
from pathlib import Path


FPS = 24
SIZE = (1600, 1256)
SOURCES = {
    "board": "Запись 2026-08-29 084517.mp4",
    "mesh": "Запись 2026-08-29 084919.mp4",
    "plugins": "Запись 2026-08-29 085206.mp4",
    "mesh-talk": "Запись 2026-08-29 085542.mp4",
}
COMMON_CROP = (350, 60, 1680, 1320)


def segment(start, end, duration, crop=COMMON_CROP, masks=(), blurs=(), preserves=(), freeze=False):
    return dict(start=start, end=end, duration=duration, crop=crop,
                masks=list(masks), blurs=list(blurs), preserves=list(preserves), freeze=freeze)


def mesh_masks(cards):
    masks = []
    for x, y, w, h in cards:
        masks.append((x + 215, y + 2, w - 215, 35))
        masks.append((x + 4, y + 39, w - 8, h - 78))
    return masks


MESH_STILL = mesh_masks([
    (902, 181, 350, 150), (538, 356, 350, 150), (1268, 356, 350, 150),
    (902, 632, 350, 181), (448, 750, 350, 150), (1358, 725, 350, 200),
    (700, 1062, 350, 154), (1106, 1062, 350, 154),
])
TALK_MAP = mesh_masks([
    (1024, 232, 350, 151), (660, 406, 350, 151), (1388, 406, 350, 151),
    (1024, 684, 350, 183), (570, 804, 350, 151), (1480, 777, 350, 204),
    (823, 1116, 350, 157), (1227, 1116, 350, 157),
])
TALK_CHAT_MASKS = [(510, 126, 1380, 52), (678, 347, 1182, 66), (580, 675, 1275, 73)]

EDITS = {
    "board": [segment(0.2, 7.2, 7, crop=(350, 300, 1680, 1010), blurs=[
        (378, 365, 279, 957), (711, 365, 281, 271), (1044, 365, 287, 957),
        (1381, 365, 283, 957), (1715, 365, 281, 957),
    ], preserves=[(385, 456, 65, 26), (719, 456, 85, 26), (1054, 456, 80, 26),
                  (1389, 456, 69, 26), (1723, 456, 49, 26)])],
    "mesh": [
        segment(0, 1 / 30, 2, masks=MESH_STILL, freeze=True),
        segment(2.95, 4.75, 4, crop=(700, 340, 1040, 816), masks=[
            (1300, 505, 215, 80), (790, 638, 670, 184),
        ]),
        segment(6.7, 7.4, 4, crop=(495, 45, 1460, 1310), masks=[
            (495, 270, 334, 1050), (835, 147, 1090, 519), (835, 730, 1090, 455),
            (835, 1185, 1090, 65),
        ]),
        segment(0, 1 / 30, 2, masks=MESH_STILL, freeze=True),
    ],
    "plugins": [
        segment(1.4, 5.6, 3, masks=[(360, 1120, 540, 196)]),
        segment(7.5, 8.5, 2, masks=[(360, 1120, 540, 196)]),
        segment(8.75, 10.65, 2, masks=[(360, 1120, 540, 196), (939, 310, 1070, 275)]),
        segment(11.0, 14.9, 3, masks=[(360, 1120, 540, 196)]),
    ],
    "mesh-talk": [
        segment(16, 19, 3, masks=TALK_CHAT_MASKS[:2]),
        segment(34, 42, 3, masks=TALK_CHAT_MASKS),
        segment(46, 64, 3, masks=TALK_CHAT_MASKS),
        segment(68, 80, 7, masks=TALK_MAP),
        segment(95, 97.5, 5, masks=[(587, 395, 1265, 65), (515, 868, 1350, 310),
                                   (515, 1243, 1350, 15)]),
        segment(139, 146, 5, masks=[(515, 110, 1350, 350), (586, 650, 1265, 40)]),
    ],
}
CAPTIONS = {
    "board": [dict(start=0, end=7, text="One board for the whole team")],
    "mesh": [dict(start=0, end=2, text="Your machines, connected"),
             dict(start=2, end=6, text="Open a peer"),
             dict(start=6, end=10, text="The same workspace in your browser"),
             dict(start=10, end=12, text="One shared mesh")],
    "plugins": [dict(start=0, end=3, text="Choose your agent"),
                dict(start=3, end=5, text="Add capabilities"),
                dict(start=5, end=7, text="Connect your secret store"),
                dict(start=7, end=10, text="Extend your workspace")],
    "mesh-talk": [dict(start=0, end=3, text="Ask your agent about the mesh"),
                  dict(start=3, end=6, text="Prepare one check-in"),
                  dict(start=6, end=9, text="Contact the online machines"),
                  dict(start=9, end=16, text="Watch messages cross the mesh"),
                  dict(start=16, end=21, text="Replies arrive"),
                  dict(start=21, end=26, text="One fleet summary")],
}
POSTERS = {"board": 2, "mesh": 10.5, "plugins": 1.6, "mesh-talk": 13}


def run(args):
    subprocess.run([str(a) for a in args], check=True)


def probe(path):
    return json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-show_format", "-show_streams", "-of", "json", str(path)
    ]))


def filter_graph(edit):
    parts = []
    last = "0:v"
    if edit["preserves"]:
        outputs = "".join(f"[k{i}]" for i in range(len(edit["preserves"])))
        parts.append(f"[0:v]split={len(edit['preserves']) + 1}[blurbase]{outputs}")
        last = "blurbase"
    for i, (x, y, w, h) in enumerate(edit["blurs"]):
        parts.append(f"[{last}]split[b{i}][r{i}]")
        parts.append(f"[r{i}]crop={w}:{h}:{x}:{y},boxblur=22:3[p{i}]")
        parts.append(f"[b{i}][p{i}]overlay={x}:{y}[s{i}]")
        last = f"s{i}"
    for i, (x, y, w, h) in enumerate(edit["preserves"]):
        parts.append(f"[k{i}]crop={w}:{h}:{x}:{y}[keep{i}]")
        parts.append(f"[{last}][keep{i}]overlay={x}:{y}[restored{i}]")
        last = f"restored{i}"
    filters = [f"drawbox=x={x}:y={y}:w={w}:h={h}:color=0x141922:t=fill"
               for x, y, w, h in edit["masks"]]
    x, y, w, h = edit["crop"]
    filters.append(f"crop={w}:{h}:{x}:{y}")
    if edit["freeze"]:
        filters.append(f"tpad=stop_mode=clone:stop_duration={edit['duration']}")
        factor = 1
    else:
        factor = edit["duration"] / (edit["end"] - edit["start"])
    filters.extend([
        f"setpts={factor:.12f}*(PTS-STARTPTS)", f"fps={FPS}",
        f"scale={SIZE[0]}:{SIZE[1]}:force_original_aspect_ratio=decrease:force_divisible_by=2",
        f"pad={SIZE[0]}:{SIZE[1]}:(ow-iw)/2:(oh-ih)/2:color=0x0d1017",
        "setsar=1", "format=yuv420p",
    ])
    parts.append(f"[{last}]" + ",".join(filters) + "[out]")
    return ";".join(parts)


def validate(path, expected_duration):
    info = probe(path)
    assert len(info["streams"]) == 1, f"Unexpected audio or extra streams: {path}"
    stream = info["streams"][0]
    assert (stream["width"], stream["height"]) == SIZE
    assert stream["pix_fmt"] == "yuv420p"
    assert abs(float(info["format"]["duration"]) - expected_duration) < 0.06
    run(["ffmpeg", "-hide_banner", "-v", "error", "-xerror", "-i", path, "-f", "null", "-"])
    return float(info["format"]["duration"])


def build(source_dir, output_dir):
    output_dir.mkdir(parents=True, exist_ok=True)
    manifest = dict(schemaVersion=1, totalBytes=0, clips=[])
    with tempfile.TemporaryDirectory(prefix="domios-media-") as temporary:
        temp = Path(temporary)
        for name, edits in EDITS.items():
            source = source_dir / SOURCES[name]
            if not source.is_file():
                raise FileNotFoundError(source)
            segments = []
            for i, edit in enumerate(edits):
                target = temp / f"{name}-{i}.mkv"
                print(f"Redact {name} segment {i + 1}/{len(edits)}", flush=True)
                run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-ss", edit["start"],
                     "-t", edit["end"] - edit["start"], "-i", source,
                     "-filter_complex", filter_graph(edit), "-map", "[out]", "-an",
                     "-t", edit["duration"], "-c:v", "libx264", "-preset", "veryfast",
                     "-crf", "16", "-threads", "8", "-map_metadata", "-1", target])
                segments.append(target)
            concat = temp / f"{name}.txt"
            concat.write_text("".join(f"file '{p.as_posix()}'\n" for p in segments), encoding="utf-8")
            common = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "concat",
                      "-safe", "0", "-i", concat, "-an", "-map_metadata", "-1"]
            mp4, webm, poster = (output_dir / f"{name}{suffix}"
                                 for suffix in (".mp4", ".webm", "-poster.webp"))
            print(f"Encode {name} H.264 / VP9", flush=True)
            run(common + ["-c:v", "libx264", "-preset", "slow", "-crf", "26", "-pix_fmt", "yuv420p",
                          "-threads", "8", "-movflags", "+faststart", mp4])
            run(common + ["-c:v", "libvpx-vp9", "-crf", "36", "-b:v", "0", "-cpu-used", "4",
                          "-row-mt", "1", "-tile-columns", "2", "-threads", "8", webm])
            run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-ss", POSTERS[name],
                 "-i", mp4, "-frames:v", "1", "-c:v", "libwebp", "-quality", "82",
                 "-map_metadata", "-1", poster])
            duration = sum(edit["duration"] for edit in edits)
            duration = validate(mp4, duration)
            validate(webm, duration)
            files = {kind: dict(src=f"/media/{path.name}", bytes=path.stat().st_size)
                     for kind, path in [("mp4", mp4), ("webm", webm), ("poster", poster)]}
            manifest["clips"].append(dict(id=name, duration=duration, width=SIZE[0], height=SIZE[1],
                                          loop=True, captions=CAPTIONS[name], **files))
            print(name, duration, sum(item["bytes"] for item in files.values()), flush=True)
    (output_dir / "captions.json").write_text(json.dumps(CAPTIONS, indent=2) + "\n", encoding="utf-8")
    manifest_path = output_dir / "manifest.json"
    for _ in range(4):
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        manifest["totalBytes"] = sum(p.stat().st_size for p in output_dir.iterdir() if p.is_file())
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    assert manifest["totalBytes"] < 15_000_000, "Media exceeds the 15 MB budget"
    print(f"Complete: {manifest['totalBytes']:,} bytes", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, default=Path.home() / "Desktop")
    parser.add_argument("--output-dir", type=Path, default=Path(__file__).resolve().parents[1] / "public/media")
    args = parser.parse_args()
    build(args.source_dir, args.output_dir)
