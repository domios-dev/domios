#!/usr/bin/env python3
"""Render the Domios trailer from read-only recordings (FFmpeg 7+, Pillow 10+)."""

import argparse
import json
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

from trailer_scenes import FPS, SIZE, render, source_frames, SHOTS


SOURCES = {
    "board": "Запись 2026-08-29 084517.mp4",
    "mesh": "Запись 2026-08-29 084919.mp4",
    "plugins": "Запись 2026-08-29 085206.mp4",
    "mesh-talk": "Запись 2026-08-29 085542.mp4",
}
CHAPTERS = [
    dict(id="agents", start=0, end=8, posterTime=3.2),
    dict(id="board", start=8, end=15, posterTime=11),
    dict(id="mesh", start=15, end=30, posterTime=18),
    dict(id="plugins", start=30, end=45, posterTime=32),
]
CAPTIONS = [
    (0, 4, "A desktop app. A place for your agents."),
    (4, 8, "Named agents live in tabs, with a conversation to return to."),
    (8, 11, "Give a task to a team of its own."),
    (11, 15, "Follow the work from pending to done."),
    (15, 19, "Pair the computers you own into one trusted mesh."),
    (19, 23, "Reach a workspace on another machine."),
    (23, 27, "Ask your agent to check in with its peers."),
    (27, 30, "Their reports come back to your conversation."),
    (30, 33, "Choose the intelligence that fits your work."),
    (33, 35.5, "Bring your tools into the workspace."),
    (35.5, 38, "Add capabilities through plugins."),
    (38, 41, "One view of the fleet. Including who is offline."),
    (41, 45, "Your next idea. A whole team behind it."),
]


def run(args):
    subprocess.run([str(arg) for arg in args], check=True)


def validate(path):
    info = json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-show_format", "-show_streams", "-of", "json", str(path)
    ]))
    assert len(info["streams"]) == 1
    stream = info["streams"][0]
    assert (stream["width"], stream["height"]) == SIZE
    assert stream["pix_fmt"] == "yuv420p"
    assert abs(float(info["format"]["duration"]) - 45) < .06
    run(["ffmpeg", "-v", "error", "-xerror", "-i", path, "-f", "null", "-"])


def still(video, time, target, width):
    run(["ffmpeg", "-v", "error", "-y", "-ss", time, "-i", video, "-frames:v", "1",
         "-vf", f"scale={width}:-2:flags=lanczos", "-c:v", "libwebp", "-quality", "84", target])


def default_font():
    candidates = ["C:/Windows/Fonts/segoeui.ttf", "/System/Library/Fonts/Supplemental/Arial.ttf",
                  "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"]
    return next((Path(path) for path in candidates if Path(path).is_file()), None)


def build(source_dir, output_dir, font, preview_dir):
    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="domios-trailer-") as temporary:
        temp = Path(temporary)
        source_paths = {key: source_dir / name for key, name in SOURCES.items()}
        for path in source_paths.values():
            if not path.is_file():
                raise FileNotFoundError(path)
        master = temp / "master.mp4"
        encoder = subprocess.Popen([
            "ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24",
            "-s", f"{SIZE[0]}x{SIZE[1]}", "-r", str(FPS), "-i", "-", "-an",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "16", "-threads", "6",
            "-pix_fmt", "yuv420p", str(master)
        ], stdin=subprocess.PIPE)
        try:
            for shot in SHOTS:
                print(f"Render {shot['at']:04.1f}s: {shot['name']}", flush=True)
                for index, frame in enumerate(source_frames(source_paths, shot)):
                    image = render(shot, index / FPS, frame, font)
                    encoder.stdin.write(image.tobytes())
            encoder.stdin.close()
            if encoder.wait() != 0:
                raise RuntimeError("Trailer render failed")
        finally:
            if encoder.poll() is None:
                encoder.terminate()
                encoder.wait()
        mp4, webm = output_dir / "trailer.mp4", output_dir / "trailer.webm"
        common = ["ffmpeg", "-v", "error", "-y", "-i", master, "-an", "-map_metadata", "-1"]
        print("Encode H.264", flush=True)
        run(common + ["-c:v", "libx264", "-preset", "slow", "-crf", "27", "-threads", "6",
                      "-pix_fmt", "yuv420p", "-movflags", "+faststart", mp4])
        print("Encode VP9", flush=True)
        run(common + ["-c:v", "libvpx-vp9", "-crf", "39", "-b:v", "0", "-cpu-used", "4",
                      "-row-mt", "1", "-threads", "6", webm])
        for path in [mp4, webm]:
            validate(path)
        still(mp4, 18, output_dir / "trailer-poster.webp", SIZE[0])
        for chapter in CHAPTERS:
            still(mp4, chapter["posterTime"], output_dir / f"{chapter['id']}-poster.webp", 480)
        captions = [dict(start=start, end=end, text=text) for start, end, text in CAPTIONS]
        manifest = dict(schemaVersion=2, duration=45, width=SIZE[0], height=SIZE[1], chapters=CHAPTERS)
        for kind, path in [("mp4", mp4), ("webm", webm), ("poster", output_dir / "trailer-poster.webp")]:
            manifest[kind] = dict(src=f"/media/{path.name}", bytes=path.stat().st_size)
        (output_dir / "captions.json").write_text(json.dumps(captions, indent=2) + "\n", encoding="utf-8")
        for name in SOURCES:
            for suffix in [".mp4", ".webm"]:
                (output_dir / f"{name}{suffix}").unlink(missing_ok=True)
        (output_dir / "mesh-talk-poster.webp").unlink(missing_ok=True)
        manifest_path = output_dir / "manifest.json"
        media_bytes = sum(p.stat().st_size for p in output_dir.iterdir() if p.is_file() and p != manifest_path)
        manifest['totalBytes'] = media_bytes
        for _ in range(3):
            manifest['totalBytes'] = media_bytes + len((json.dumps(manifest, indent=2) + '\n').encode('utf-8'))
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline='\n')
        assert manifest['totalBytes'] == sum(p.stat().st_size for p in output_dir.iterdir() if p.is_file())
        assert manifest['totalBytes'] < 6_000_000, 'Trailer exceeds the 6 MB media budget'
        print(f"Media: {manifest['totalBytes']:,} bytes", flush=True)
        if preview_dir:
            preview_dir.mkdir(parents=True, exist_ok=True)
            run(["ffmpeg", "-v", "error", "-y", "-i", mp4, "-filter_complex",
                 "fps=8,scale=640:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer",
                 "-loop", "0", preview_dir / "trailer-preview.gif"])
            for i, time in enumerate([2.5, 6, 11, 18, 32, 42.5]):
                run(["ffmpeg", "-v", "error", "-y", "-ss", time, "-i", mp4,
                     "-frames:v", "1", preview_dir / f"key-{i + 1}.png"])
            sheet = Image.new("RGB", (1280, 1080))
            for i in range(6):
                frame = Image.open(preview_dir / f"key-{i + 1}.png").resize((640, 360))
                sheet.paste(frame, ((i % 2) * 640, (i // 2) * 360))
            sheet.save(preview_dir / "storyboard.jpg", quality=90)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, default=Path.home() / "Desktop")
    parser.add_argument("--output-dir", type=Path, default=Path(__file__).resolve().parents[1] / "public/media")
    parser.add_argument("--font", type=Path, default=default_font())
    parser.add_argument("--preview-dir", type=Path)
    args = parser.parse_args()
    if args.font is None or not args.font.is_file():
        parser.error('Provide an installed TrueType font with --font')
    build(args.source_dir, args.output_dir, args.font, args.preview_dir)
