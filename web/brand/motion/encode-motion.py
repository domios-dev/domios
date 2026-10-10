from pathlib import Path
import argparse
import json
import subprocess

from PIL import Image


def encode(root):
    manifest = json.loads((root / "capture-manifest.json").read_text(encoding="utf-8"))
    width, height, fps = manifest["frameWidth"], manifest["frameHeight"], manifest["fps"]
    delivery = []
    for clip in manifest["clips"]:
        mp4 = root / f'{clip["kind"]}.mp4'
        gif = root / f'{clip["kind"]}.gif'
        command = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24",
                   "-s", f"{width}x{height}", "-r", str(fps), "-i", "pipe:0", "-an", "-c:v", "libx264",
                   "-preset", "medium", "-crf", "20", "-threads", "4", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(mp4)]
        process = subprocess.Popen(command, stdin=subprocess.PIPE)
        try:
            for entry in clip["files"]:
                with Image.open(entry["file"]) as atlas:
                    for frame in range(entry["frames"]):
                        x, y = frame % 2 * width, frame // 2 * height
                        process.stdin.write(atlas.crop((x, y, x + width, y + height)).convert("RGB").tobytes())
        finally:
            process.stdin.close()
        if process.wait(timeout=60) != 0:
            raise RuntimeError(f"MP4 encoding failed: {clip['kind']}")
        filters = "fps=20,scale=480:360:flags=lanczos,split[v][p];[p]palettegen=max_colors=64:stats_mode=diff[palette];[v][palette]paletteuse=dither=bayer:bayer_scale=3"
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(mp4), "-filter_complex", filters,
                        "-loop", "0", str(gif)], check=True, timeout=60)
        record = dict(id=clip["kind"], duration=clip["duration"], mp4=str(mp4), gif=str(gif),
                      mp4Bytes=mp4.stat().st_size, gifBytes=gif.stat().st_size)
        if max(record["mp4Bytes"], record["gifBytes"]) >= 3_000_000:
            raise ValueError(f"Preview exceeds 3 MB: {record}")
        info = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", str(mp4)]))
        stream = info["streams"][0]
        if (stream["width"], stream["height"]) != (width, height) or abs(float(info["format"]["duration"]) - clip["duration"]) > .05:
            raise ValueError(f"Unexpected video dimensions or duration: {mp4}")
        subprocess.run(["ffmpeg", "-hide_banner", "-v", "error", "-xerror", "-i", str(mp4), "-f", "null", "-"], check=True, timeout=60)
        with Image.open(gif) as image:
            record["gifFrames"] = image.n_frames
            record["gifSize"] = image.size
        delivery.append(record)
        print(record, flush=True)
    (root / "previews.json").write_text(json.dumps(delivery, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Encode captured Domios motion frames into compact MP4/GIF previews.")
    parser.add_argument("output", type=Path)
    encode(parser.parse_args().output)
