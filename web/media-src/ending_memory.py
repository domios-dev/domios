import hashlib
import math
import subprocess
from pathlib import Path

from PIL import Image
from media_contract import bounded_process, probe, require

MEMORY_SHA256 = 'b99f5e730a844ef2d7d69f2f5f0acb54b9be2ec212c22d220af7f729c25f4393'
MEMORY_START_FRAME = 1112
MEMORY_END_FRAME = 1216
MEMORY_SOURCE_START = 6.2
FRAME_SIZE = (2048, 1396)


def memory_source(source_dir):
    path = Path(source_dir) / 'memory.mp4'
    require(hashlib.sha256(path.read_bytes()).hexdigest() == MEMORY_SHA256,
            'Owner Memory recording changed')
    streams = [stream for stream in probe(path)['streams'] if stream['codec_type'] == 'video']
    require(len(streams) == 1 and streams[0]['avg_frame_rate'] == '30/1', 'Memory must be native30')
    require((streams[0]['width'], streams[0]['height']) == (2534, 1756), 'Memory dimensions changed')
    require(float(streams[0]['duration']) >= MEMORY_SOURCE_START + (MEMORY_END_FRAME - MEMORY_START_FRAME) / 30,
            'Memory source range exceeds recording')
    return path


def memory_frames(path, first_output_frame):
    offset = first_output_frame - MEMORY_START_FRAME
    count = MEMORY_END_FRAME - first_output_frame
    require(0 <= offset < MEMORY_END_FRAME - MEMORY_START_FRAME, 'Memory output range invalid')
    source_frame = round(MEMORY_SOURCE_START * 30) + offset
    command = ['ffmpeg', '-v', 'error', '-nostdin', '-ss', str(source_frame / 30), '-i', str(path),
               '-vf', 'scale=2048:1396:force_original_aspect_ratio=decrease,pad=2048:1396:(ow-iw)/2:(oh-ih)/2',
               '-fps_mode', 'passthrough', '-frames:v', str(count), '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-']
    with bounded_process(command, 180, stdout=subprocess.PIPE) as process:
        try:
            for index in range(count):
                data = process.stdout.read(math.prod(FRAME_SIZE) * 3)
                require(len(data) == math.prod(FRAME_SIZE) * 3, f'Incomplete Memory frame {source_frame + index}')
                yield Image.frombytes('RGB', FRAME_SIZE, data)
        finally:
            while process.stdout.read(math.prod(FRAME_SIZE) * 3):
                pass
            require(process.wait(timeout=180) == 0, 'Memory decode failed')
