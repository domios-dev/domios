"""Create phrase-led timing and word provenance from the single complete voice take."""

import argparse
import hashlib
import json
import re
from pathlib import Path

from media_contract import CHAPTERS, read_timing, require, write_json


def prepare(source_metadata, alignment_path, audio, output, offset, duration):
    source = json.loads(source_metadata.read_text(encoding='utf-8-sig'))
    response = json.loads(alignment_path.read_text(encoding='utf-8-sig'))
    alignment = response['alignment']
    digest = hashlib.sha256(audio.read_bytes()).hexdigest()
    require(digest.lower() == source['sha256'].lower(), 'Voice hash differs from supplied provenance')
    text = ''.join(alignment['characters'])
    starts, ends = alignment['character_start_times_seconds'], alignment['character_end_times_seconds']
    require(len(text) == len(starts) == len(ends), 'Character timestamp count mismatch')
    require(all(a <= b for a, b in zip(starts, ends)), 'Reversed character timestamps')
    require(all(a <= b for a, b in zip(starts, starts[1:])), 'Non-monotonic alignment')
    phrases = source['phrases']
    require(len(phrases) == 13 and [phrase['id'] for phrase in phrases] == list(range(1, 14)), 'Expected13 phrases')
    position = 0
    words = []
    cues = []
    for phrase in phrases:
        while position < len(text) and text[position].isspace():
            position += 1
        require(text.startswith(phrase['text'], position), f'Phrase text differs from actual alignment: {phrase["id"]}')
        end = position + len(phrase['text'])
        for match in re.finditer(r'\S+', phrase['text']):
            first, last = position + match.start(), position + match.end() - 1
            words.append(dict(cueId=phrase['id'], text=match.group(), sourceStart=starts[first], sourceEnd=ends[last],
                              finalStart=starts[first] + offset, finalEnd=ends[last] + offset, speed=1))
        cues.append(dict(id=phrase['id'], text=phrase['text'], start=starts[position] + offset, end=ends[end - 1] + offset))
        position = end
    require(not text[position:].strip(), 'Unmapped spoken text remains')
    require(source['charactersTotal'] <= 3000 and source['requestsTotal'] == 2, 'Initial paid request allowance exceeded')
    require(offset >= .25 and cues[-1]['end'] + .5 <= duration, 'Need establishment and complete final hold')
    boundaries = [0] + [round((a['end'] + b['start']) / 2 * 30) / 30 for a, b in zip(cues, cues[1:])] + [duration]
    for index, cue in enumerate(cues):
        cue.update(visualStart=boundaries[index], visualEnd=boundaries[index + 1])
    chapter_starts = [boundaries[index] for index in (0, 2, 4, 8)]
    chapters = []
    for index, chapter in enumerate(CHAPTERS):
        start = chapter_starts[index]
        end = chapter_starts[index + 1] if index < 3 else duration
        opening_cue = cues[(0, 2, 4, 8)[index]]
        poster_time = start + (opening_cue['visualEnd'] - start) * .8
        chapters.append(dict(id=chapter['id'], start=start, end=end,
                             posterTime=min(opening_cue['visualEnd'] - 1 / 30, poster_time)))
    timing = dict(schemaVersion=1, fps=30, duration=duration, chapters=chapters, cues=cues,
                  narration=dict(audioSha256=digest, alignmentSha256=hashlib.sha256(alignment_path.read_bytes()).hexdigest(),
                                 sourceDuration=source['duration'], offset=offset, speed=1,
                                 voice=source['voice'], model=source['model'],
                                 charactersTotal=source['charactersTotal'], requestsTotal=source['requestsTotal'],
                                 method='Complete-take service character timestamps; unchanged continuous audio plus initial silence',
                                 words=words))
    require(not output.exists(), 'Timing output already exists; preserve review evidence')
    require('trailer-before-' not in str(output.resolve()), 'Preserved baseline is read-only')
    require((Path(__file__).resolve().parents[1] / 'public/media') not in output.resolve().parents, 'Timing provenance stays private')
    output.parent.mkdir(parents=True, exist_ok=True)
    write_json(output, timing)
    read_timing(output)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('source-metadata', 'alignment', 'audio', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--offset', type=float, default=.65)
    parser.add_argument('--duration', type=float, default=45)
    args = parser.parse_args()
    prepare(args.source_metadata, args.alignment, args.audio, args.output, args.offset, args.duration)
