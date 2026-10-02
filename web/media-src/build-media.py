#!/usr/bin/env python3
"""Build landing-page media with FFmpeg 7.1+ and Python 3.12+."""

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


def segment(start, end, duration, crop=COMMON_CROP, freeze=False):
    return dict(start=start, end=end, duration=duration, crop=crop, freeze=freeze)


EDITS = {
    "board": [segment(0.2, 7.2, 7, crop=(350, 300, 1680, 1010))],
    "mesh": [
        segment(0, 1 / 30, 2, freeze=True),
        segment(2.95, 4.75, 4, crop=(700, 340, 1040, 816)),
        segment(6.7, 7.4, 4, crop=(495, 45, 1460, 1310)),
        segment(0, 1 / 30, 2, freeze=True),
    ],
    "plugins": [
        segment(1.4, 5.6, 3),
        segment(7.5, 8.5, 2),
        segment(8.75, 10.65, 2),
        segment(11.0, 14.9, 3),
    ],
    "mesh-talk": [
        segment(16, 19, 3),
        segment(34, 42, 3),
        segment(46, 64, 3),
        segment(68, 80, 7),
        segment(95, 97.5, 5),
        segment(139, 146, 5),
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
    filters = []
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
    return "[0:v]" + ",".join(filters) + "[out]"


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
                print(f"Cut {name} segment {i + 1}/{len(edits)}", flush=True)
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
