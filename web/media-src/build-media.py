#!/usr/bin/env python3
"""Render a private review candidate from read-only recordings."""

import argparse
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

from media_contract import SOURCES, bounded_process, prepare_output, read_timing
from media_contract import probe, require, run, write_captions, write_json, write_manifest
from timeline_audit import timeline_audit
from narration_proof import sha256
from ending_memory import (MEMORY_START_FRAME, MEMORY_END_FRAME, MEMORY_SHA256,
                           MEMORY_SOURCE_START, memory_source, memory_frames)
from trailer_scenes import (ENDING_END_FRAME, ENDING_START_FRAME, FPS, OPENING_FRAMES, SIZE,
                            ENDING_MESH_PATH, ENDING_MESH_SHA256,
                            load_brand_frames, render_timeline_frame, source_frames, timed_shots)


def still(video, time, target, width):
    run(['ffmpeg', '-v', 'error', '-y', '-ss', time, '-i', video, '-frames:v', '1',
         '-vf', f'scale={width}:-2:flags=lanczos', '-c:v', 'libwebp', '-quality', '84', target])


def default_font():
    candidates = ['C:/Windows/Fonts/segoeui.ttf', '/System/Library/Fonts/Supplemental/Arial.ttf',
                  '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf']
    return next((Path(path) for path in candidates if Path(path).is_file()), None)


def validate_sources(paths, shots):
    for key, path in paths.items():
        require(path.is_file(), f'Missing source: {path}')
        stream = next(s for s in probe(path)['streams'] if s['codec_type'] == 'video')
        require(stream['avg_frame_rate'] == '30/1', f'Non-native30 source: {key}')
        require(stream['width'] >= 2048 and stream['height'] >= 1396, f'Source crop too small: {key}')
        require(max(edit['end'] for edit in shots if edit['source'] == key) <= float(stream['duration']),
                f'Source range beyond duration: {key}')


def build(source_dir, output_dir, font, preview_dir, brand_directory, review_range=None, timing_path=None):
    timing = read_timing(timing_path)
    shots = timed_shots(timing)
    brand_frames = load_brand_frames(brand_directory)
    if review_range:
        require(review_range[1] <= timing['duration'], 'Review range beyond chosen duration')
    prepare_output(output_dir, [source_dir, brand_directory])
    if preview_dir:
        prepare_output(preview_dir, [source_dir, output_dir, brand_directory])
    source_paths = {key: source_dir / name for key, name in SOURCES.items()}
    validate_sources(source_paths, shots)
    memory_path = memory_source(source_dir)
    audit = timeline_audit('222f60c46', review_range, timing)
    audit['timingSha256'] = sha256(timing_path) if timing_path else None
    audit['framing'] = dict(brandSourceSha256=brand_frames['sourceSha256'],
                            endingMemory=dict(path=str(memory_path), sha256=MEMORY_SHA256,
                                              outputFrames=[MEMORY_START_FRAME, MEMORY_END_FRAME],
                                              sourceStart=MEMORY_SOURCE_START, nativeFps=FPS),
                            endingScreenshot=dict(path=str(ENDING_MESH_PATH), sha256=ENDING_MESH_SHA256,
                                                  firstFrame=1140, crop=[334, 50, 3496, 2079]),
                            brandManifestSha256=sha256(brand_directory / 'brand-frames.json'),
                            changedFrameRanges=[[0, OPENING_FRAMES], [ENDING_START_FRAME, ENDING_END_FRAME]])
    start, end = review_range or (0, timing['duration'])
    start_frame, end_frame = round(start * FPS), round(end * FPS)
    with tempfile.TemporaryDirectory(prefix='domios-trailer-') as temporary:
        master = Path(temporary) / 'master.mp4'
        with bounded_process([
            'ffmpeg', '-v', 'error', '-y', '-f', 'rawvideo', '-pix_fmt', 'rgb24',
            '-s', f'{SIZE[0]}x{SIZE[1]}', '-r', FPS, '-i', '-', '-an',
            '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '16', '-threads', '6',
            '-pix_fmt', 'yuv420p', master], 1800, stdin=subprocess.PIPE) as encoder:
            memory_sequence = None
            try:
                for edit in shots:
                    if edit['at'] >= end or edit['at'] + edit['duration'] <= start:
                        continue
                    print(f'Render {edit["at"]:04.1f}s: {edit["name"]}', flush=True)
                    for index, frame in enumerate(source_frames(source_paths, edit)):
                        output_frame = round((edit['at'] + index / FPS) * FPS)
                        if start_frame <= output_frame < end_frame:
                            memory_frame = None
                            if MEMORY_START_FRAME <= output_frame < MEMORY_END_FRAME:
                                if memory_sequence is None:
                                    memory_sequence = memory_frames(memory_path, output_frame)
                                memory_frame = next(memory_sequence)
                            elif memory_sequence is not None:
                                memory_sequence.close()
                                memory_sequence = None
                            encoder.stdin.write(render_timeline_frame(edit, index / FPS, frame, font, brand_frames, memory_frame).tobytes())
                encoder.stdin.close()
                require(encoder.wait(timeout=1800) == 0, 'Trailer render failed')
            finally:
                if memory_sequence is not None:
                    memory_sequence.close()
                if not encoder.stdin.closed:
                    encoder.stdin.close()
        mp4, webm = output_dir / 'trailer.mp4', output_dir / 'trailer.webm'
        common = ['ffmpeg', '-v', 'error', '-y', '-i', master, '-an', '-map_metadata', '-1']
        run(common + ['-c:v', 'libx264', '-preset', 'slow', '-crf', '27', '-threads', '6',
                      '-pix_fmt', 'yuv420p', '-movflags', '+faststart', mp4], timeout=1800)
        run(common + ['-c:v', 'libvpx-vp9', '-crf', '40', '-b:v', '0', '-cpu-used', '4',
                      '-row-mt', '1', '-threads', '6', webm], timeout=1800)
    if review_range:
        write_json(output_dir / 'review-audit.json', audit)
        print('Review-only output; no publishable manifest', flush=True)
        return
    still(mp4, timing['chapters'][2]['posterTime'], output_dir / 'trailer-poster.webp', SIZE[0])
    for chapter in timing['chapters']:
        still(mp4, chapter['posterTime'], output_dir / f'{chapter["id"]}-poster.webp', 480)
    captions = [{key: cue[key] for key in ('id', 'start', 'end', 'text')} for cue in timing['cues']]
    write_captions(output_dir, captions, timing)
    manifest = dict(schemaVersion=2, duration=timing['duration'], fps=FPS, width=SIZE[0], height=SIZE[1],
                    chapters=timing['chapters'], framing=audit['framing'])
    manifest['timingSha256'] = audit['timingSha256']
    for kind, path in [('mp4', mp4), ('webm', webm), ('poster', output_dir / 'trailer-poster.webp')]:
        manifest[kind] = dict(src=f'/media/{path.name}', bytes=path.stat().st_size)
    write_manifest(output_dir, manifest)
    if preview_dir:
        preview_dir.mkdir(parents=True, exist_ok=True)
        write_json(preview_dir / 'timeline.json', audit)
        run(['ffmpeg', '-v', 'error', '-y', '-i', mp4, '-filter_complex',
             'fps=8,scale=640:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer',
             '-loop', '0', preview_dir / 'trailer-preview.gif'])
        sheet = Image.new('RGB', (1280, 1080))
        times = [timing['cues'][index]['visualStart'] + (timing['cues'][index]['visualEnd'] - timing['cues'][index]['visualStart']) * .7 for index in (0, 1, 3, 4, 8, 12)]
        for index, time in enumerate(times):
            path = preview_dir / f'key-{index + 1}.png'
            run(['ffmpeg', '-v', 'error', '-y', '-ss', time, '-i', mp4, '-frames:v', '1', path])
            with Image.open(path) as frame:
                sheet.paste(frame.resize((640, 360)), ((index % 2) * 640, (index // 2) * 360))
        sheet.save(preview_dir / 'storyboard.jpg', quality=90)
    print(f'Media: {manifest["totalBytes"]:,} bytes', flush=True)


def review_range(value):
    start, end = map(float, value.split(':'))
    require(0 <= start < end <= 55, 'Review range must be within0:55')
    require(all(abs(t * FPS - round(t * FPS)) < 1e-6 for t in (start, end)), 'Range must lie on native frames')
    return start, end


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-dir', type=Path, default=Path.home() / 'Desktop')
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--font', type=Path, default=default_font())
    parser.add_argument('--preview-dir', type=Path)
    parser.add_argument('--brand-frames', type=Path, required=True)
    parser.add_argument('--review-range', type=review_range)
    parser.add_argument('--timing', type=Path)
    args = parser.parse_args()
    if args.font is None or not args.font.is_file():
        parser.error('Provide an installed TrueType font with --font')
    build(args.source_dir, args.output_dir, args.font, args.preview_dir, args.brand_frames,
          args.review_range, args.timing)
