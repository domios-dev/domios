import hashlib
import json
import re

from media_contract import require


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def verify_words(timing):
    narration = timing['narration']
    require(narration['speed'] == 1, 'This complete-take path preserves speech speed exactly')
    require(isinstance(narration['requestsTotal'], int) and narration['requestsTotal'] > 0
            and isinstance(narration['charactersTotal'], int) and narration['charactersTotal'] > 0,
            'Missing actual synthesis request/character accounting')
    previous_source = 0
    previous_final = 0
    for cue in timing['cues']:
        words = [word for word in narration['words'] if word['cueId'] == cue['id']]
        require([word['text'] for word in words] == re.findall(r'\S+', cue['text']), 'Transcript differs from aligned words')
        require(abs(words[0]['finalStart'] - cue['start']) <= .150, 'Cue onset not synchronized')
        for word in words:
            require(previous_source <= word['sourceStart'] <= word['sourceEnd'], 'Invalid source word order')
            require(previous_final <= word['finalStart'] < word['finalEnd'] <= cue['end'] + 1e-6, 'Word crosses cue end')
            require(word['finalStart'] >= cue['start'] - 1e-6, 'Word precedes its caption')
            for key in ('Start', 'End'):
                require(abs(word[f'final{key}'] - word[f'source{key}'] - narration['offset']) < 1e-6,
                        'Word map differs from actual complete-take offset')
            previous_source, previous_final = word['sourceEnd'], word['finalEnd']
    require(len(narration['words']) == sum(len(re.findall(r'\S+', cue['text'])) for cue in timing['cues']), 'Unmapped words')
    require(previous_source <= narration['sourceDuration'], 'Word beyond recording')
    require(narration['sourceDuration'] + narration['offset'] <= timing['duration'], 'Voice would be truncated')


def verify_character_alignment(timing, path):
    require(sha256(path) == timing['narration']['alignmentSha256'], 'Original alignment changed')
    alignment = json.loads(path.read_text(encoding='utf-8-sig'))['alignment']
    text = ''.join(alignment['characters'])
    starts, ends = alignment['character_start_times_seconds'], alignment['character_end_times_seconds']
    matches = list(re.finditer(r'\S+', text))
    words = timing['narration']['words']
    require(len(matches) == len(words), 'Character alignment has missing/unmapped words')
    for match, word in zip(matches, words):
        require(match.group() == word['text'], 'Words differ from original generated character alignment')
        require(starts[match.start()] == word['sourceStart'] and ends[match.end() - 1] == word['sourceEnd'],
                'Word source timestamp differs from original alignment')
