import argparse
import hashlib
import shutil
import subprocess
import tempfile
from pathlib import Path

from media_contract import bounded_process, prepare_output, read_timing, require, run, write_captions, write_json, write_manifest
from memory_extension import INSERT_FRAME, INSERT_FRAMES, computer_finale, decoded_frames, load, memory_picture, record
from ending_memory import MEMORY_SHA256, memory_source
from trailer_scenes import SIZE, load_brand_frames


def render(baseline, prepared, source_dir, brand_frames, font, output):
    timing_path = prepared / 'timing.json'
    timing = read_timing(timing_path)
    require(len(timing['cues']) == 14 and round(timing['duration'] * 30) == 1630,
            'Expected the approved fourteen-phrase Memory extension')
    prepare_output(output, [baseline, prepared, source_dir, brand_frames])
    memory = memory_source(source_dir)
    brand = load_brand_frames(brand_frames)['images'][-1]
    finale = computer_finale(font, brand)
    frame_count = round(timing['duration'] * 30)
    prefix_digest = hashlib.sha256()
    count = 0
    with tempfile.TemporaryDirectory(prefix='domios-memory-') as temporary:
        master = Path(temporary) / 'master.mp4'
        command = ['ffmpeg', '-v', 'error', '-nostdin', '-n', '-f', 'rawvideo', '-pix_fmt', 'rgb24',
                   '-s', '1280x720', '-r', '30', '-i', '-', '-an', '-c:v', 'libx264',
                   '-preset', 'veryfast', '-crf', '16', '-threads', '6', '-pix_fmt', 'yuv420p', master]
        with bounded_process(command, 600, stdin=subprocess.PIPE) as encoder:
            try:
                for frame in decoded_frames(baseline / 'trailer.mp4', INSERT_FRAME):
                    data = frame.tobytes()
                    prefix_digest.update(data)
                    encoder.stdin.write(data)
                    count += 1
                print(f'Original picture prefix consumed: {count} frames', flush=True)
                for frame in decoded_frames(memory, INSERT_FRAMES, start=6.2, size=(2048, 1396)):
                    encoder.stdin.write(memory_picture(frame, font, brand).tobytes())
                    count += 1
                for _ in range(frame_count - count):
                    encoder.stdin.write(finale.tobytes())
                    count += 1
                require(count == frame_count, 'Extended movie frame count mismatch')
                encoder.stdin.close()
                require(encoder.wait(timeout=600) == 0, 'Extended movie render failed')
            finally:
                if not encoder.stdin.closed:
                    encoder.stdin.close()
        print('Encode extended picture', flush=True)
        common = ['ffmpeg', '-v', 'error', '-nostdin', '-n', '-i', master, '-an', '-map_metadata', '-1']
        run(common + ['-c:v', 'libx264', '-preset', 'slow', '-crf', '27', '-threads', '6',
                      '-pix_fmt', 'yuv420p', '-movflags', '+faststart', output / 'trailer.mp4'], timeout=600)
        run(common + ['-c:v', 'libvpx-vp9', '-crf', '40', '-b:v', '0', '-cpu-used', '4',
                      '-row-mt', '1', '-threads', '6', output / 'trailer.webm'], timeout=600)
    for poster in baseline.glob('*-poster.webp'):
        shutil.copyfile(poster, output / poster.name)
    manifest = load(baseline / 'manifest.json')
    manifest.update(duration=timing['duration'], chapters=timing['chapters'],
                    timingSha256=record(timing_path)['sha256'])
    for key in ('music', 'musicArrangement', 'subtitles', 'hasNarration', 'framing'):
        manifest.pop(key, None)
    for kind in ('mp4', 'webm'):
        manifest[kind]['bytes'] = (output / f'trailer.{kind}').stat().st_size
    captions = [{key: cue[key] for key in ('id', 'start', 'end', 'text')} for cue in timing['cues']]
    write_captions(output, captions, timing)
    write_manifest(output, manifest)
    write_json(prepared / 'picture-extension.json', dict(
        baseline=record(baseline / 'trailer.mp4'), originalDecodedPrefixFrames=INSERT_FRAME,
        originalDecodedPrefixRgbSha256=prefix_digest.hexdigest(),
        preservation='Original decoded RGB prefix fed unchanged into the extended render; reencoded compressed streams are not claimed identical',
        memorySource=dict(path=str(memory), sha256=MEMORY_SHA256, firstSourceFrame=186,
                          sourceFrames=INSERT_FRAMES, outputFrames=[1112, 1392]),
        finaleFrames=[1392, 1630], outputFrames=count,
        outputMp4=record(output / 'trailer.mp4'), outputWebm=record(output / 'trailer.webm')))
    print(f'Extended silent movie ready: {output}', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Render owner-requested Memory episode and a complete computer finale')
    for name in ('baseline', 'prepared', 'source-dir', 'brand-frames', 'font', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    render(args.baseline, args.prepared, args.source_dir, args.brand_frames, args.font, args.output)
