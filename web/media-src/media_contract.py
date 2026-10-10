import json
import subprocess
import threading
from contextlib import contextmanager
from pathlib import Path


SOURCES = {
    'board': 'Запись 2026-08-29 084517.mp4',
    'mesh': 'Запись 2026-08-29 084919.mp4',
    'plugins': 'Запись 2026-08-29 085206.mp4',
    'mesh-talk': 'Запись 2026-08-29 085542.mp4',
}
CHAPTERS = [
    dict(id='agents', start=0, end=8, posterTime=3.2),
    dict(id='board', start=8, end=15, posterTime=11),
    dict(id='mesh', start=15, end=30, posterTime=18),
    dict(id='plugins', start=30, end=45, posterTime=32),
]
CUE_WINDOWS = [(0, 4), (4, 8), (8, 11), (11, 15), (15, 19), (19, 23), (23, 27),
               (27, 30), (30, 33), (33, 35.5), (35.5, 38), (38, 41), (41, 45)]
SILENT_TEXT = [
    'A desktop app. A place for your agents.',
    'Named agents live in tabs, with a conversation to return to.',
    'Give a task to a team of its own.',
    'Follow the work from pending to done.',
    'Pair the computers you own into one trusted mesh.',
    'Reach a workspace on another machine.',
    'Ask your agent to check in with its peers.',
    'Their reports come back to your conversation.',
    'Choose the intelligence that fits your work.',
    'Bring your tools into the workspace.',
    'Add capabilities through plugins.',
    'One view of the fleet. Including who is offline.',
    'Your next idea. A whole team behind it.',
]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def run(args, timeout=300, capture=False):
    result = subprocess.run([str(arg) for arg in args], check=True, timeout=timeout,
                            stdout=subprocess.PIPE if capture else None)
    return result.stdout if capture else None


@contextmanager
def bounded_process(args, timeout, **kwargs):
    process = subprocess.Popen([str(arg) for arg in args], **kwargs)
    expired = threading.Event()

    def expire():
        expired.set()
        process.kill()

    deadline = threading.Timer(timeout, expire)
    deadline.start()
    try:
        yield process
        require(not expired.is_set(), f'Process deadline exceeded ({timeout}s): {args[0]}')
    finally:
        deadline.cancel()
        if process.poll() is None:
            process.kill()
        process.wait(timeout=10)


def probe(path):
    return json.loads(run(['ffprobe', '-v', 'error', '-count_frames', '-show_streams',
                           '-show_format', '-of', 'json', path], timeout=120, capture=True))


def write_json(path, value):
    require('trailer-before-' not in str(path.resolve()), 'Preserved baseline is read-only')
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n', encoding='utf-8', newline='\n')


def prepare_output(path, inputs=()):
    target = path.resolve()
    public = Path(__file__).resolve().parents[1] / 'public/media'
    require(target != public.resolve(), 'Render to a private review directory; stage selected assets separately')
    require('trailer-before-' not in str(target), 'Preserved baseline is read-only')
    for source in inputs:
        resolved = source.resolve()
        require(target != resolved and target not in resolved.parents and resolved not in target.parents,
                'Output must be separate from all inputs')
    require(not target.exists() or not any(target.iterdir()), 'Output directory must be empty')
    target.mkdir(parents=True, exist_ok=True)


def write_manifest(directory, manifest):
    path = directory / 'manifest.json'
    other_bytes = sum(p.stat().st_size for p in directory.iterdir() if p.is_file() and p != path)
    manifest['totalBytes'] = other_bytes
    while True:
        encoded = (json.dumps(manifest, indent=2, ensure_ascii=False) + '\n').encode('utf-8')
        total = other_bytes + len(encoded)
        if manifest['totalBytes'] == total:
            break
        manifest['totalBytes'] = total
    path.write_bytes(encoded)


def validate_captions(captions, timing=None):
    require(bool(captions), 'Expected narration cues')
    if timing:
        require(len(captions) == len(timing['cues']), 'Caption count differs from timing authority')
    previous = 0
    for index, cue in enumerate(captions):
        require(cue.get('id', index + 1) == index + 1, 'Cue identity/order changed')
        require(previous <= cue['start'] < cue['end'], 'Overlapping/reversed cues')
        require(isinstance(cue['text'], str) and bool(cue['text'].strip()), 'Empty cue text')
        if timing:
            expected = timing['cues'][index]
            require(all(cue[key] == expected[key] for key in ('start', 'end', 'text')), 'Cue differs from timing authority')
        previous = cue['end']


def write_captions(directory, captions, timing=None):
    validate_captions(captions, timing)
    write_json(directory / 'captions.json', captions)
    write_json(directory / 'transcript.json', captions)

    def stamp(seconds):
        millis = round(seconds * 1000)
        return f'{millis // 3600000:02}:{millis // 60000 % 60:02}:{millis // 1000 % 60:02}.{millis % 1000:03}'

    vtt = 'WEBVTT\n\n' + '\n\n'.join(
        f'{index + 1}\n{stamp(cue["start"])} --> {stamp(cue["end"])}\n{cue["text"]}'
        for index, cue in enumerate(captions)) + '\n'
    (directory / 'captions.vtt').write_text(vtt, encoding='utf-8', newline='\n')


def read_timing(path=None):
    if path:
        timing = json.loads(path.read_text(encoding='utf-8-sig'))
    else:
        timing = dict(duration=45, fps=30, chapters=CHAPTERS,
                      cues=[dict(id=i + 1, start=a, end=b, visualStart=a, visualEnd=b, text=text)
                            for i, ((a, b), text) in enumerate(zip(CUE_WINDOWS, SILENT_TEXT))])
    require(timing['fps'] == 30 and 40 <= timing['duration'] <= 55, 'Expected40–55s at native30fps')
    require(abs(timing['duration'] * 30 - round(timing['duration'] * 30)) < 1e-6, 'Duration must end on a frame')
    validate_captions(timing['cues'])
    cues = timing['cues']
    require(cues[0]['visualStart'] == 0 and cues[-1]['visualEnd'] == timing['duration'], 'Visual extent mismatch')
    for index, cue in enumerate(cues):
        require(cue['visualStart'] <= cue['start'] < cue['end'] <= cue['visualEnd'], 'Phrase crosses visual cut')
        require(all(abs(cue[key] * 30 - round(cue[key] * 30)) < 1e-6 for key in ('visualStart', 'visualEnd')),
                'Visual cut must lie on a frame')
        if index:
            require(cues[index - 1]['visualEnd'] == cue['visualStart'], 'Visual timeline gap')
    chapters = timing['chapters']
    require([chapter['id'] for chapter in chapters] == ['agents', 'board', 'mesh', 'plugins'], 'Chapter IDs/order changed')
    for index, cue_index in enumerate((0, 2, 4, 8)):
        require(chapters[index]['start'] == cues[cue_index]['visualStart'], 'Chapter must start at its shot')
        expected_end = chapters[index + 1]['start'] if index < 3 else timing['duration']
        require(chapters[index]['end'] == expected_end, 'Chapter extent mismatch')
    return timing
