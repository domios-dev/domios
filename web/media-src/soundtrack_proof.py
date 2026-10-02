import math
import sys
from array import array
from pathlib import Path

from media_contract import require, run
from narration_proof import sha256


RATE = 48000
MINIMUM_SEPARATION_DB = 14


def audio_record(path):
    return dict(path=str(path.resolve()), sha256=sha256(path))


def samples(record, count):
    path = Path(record['path'])
    require(sha256(path) == record['sha256'], f'Soundtrack proof file changed: {path}')
    decoded = run(['ffmpeg', '-v', 'error', '-i', path, '-map', '0:a:0', '-ar', RATE, '-ac', '1',
                   '-c:a', 'pcm_f32le', '-f', 'f32le', '-'], timeout=120, capture=True)
    values = array('f')
    values.frombytes(decoded)
    if sys.byteorder != 'little':
        values.byteswap()
    require(values.itemsize == 4 and len(values) == count, f'Stem sample count differs from timing: {path}')
    require(all(math.isfinite(value) for value in values), f'Nonfinite stem sample: {path}')
    return values


def rms(values, start, end):
    return math.sqrt(math.fsum(values[index] ** 2 for index in range(start, end)) / (end - start))


def decibels(value):
    return 20 * math.log10(value) if value > 0 else None


def measure_balance(timing, stems, final_audio, mastering):
    count = round(timing['duration'] * RATE)
    decoded = {key: samples(record, count) for key, record in stems.items()}
    required = {'voicePostDuck', 'musicPostDuck', 'voiceMastered', 'musicMastered', 'unmasteredMix'}
    require(set(decoded) == required, 'Incomplete actual post-duck/mastered stem evidence')
    final = samples(final_audio, count)
    gain = mastering['linearGain']
    require(math.isfinite(gain) and gain > 0, 'Invalid shared mastering gain')
    require(abs(gain - 10 ** (mastering['gainDb'] / 20)) <= 1e-12, 'Mastering gain metadata disagrees')
    residuals = dict(unmasteredSum=0, voiceSharedGain=0, musicSharedGain=0, finalSum=0, finalSharedGain=0)
    for index in range(count):
        voice, music = decoded['voicePostDuck'][index], decoded['musicPostDuck'][index]
        mastered_voice, mastered_music = decoded['voiceMastered'][index], decoded['musicMastered'][index]
        unmastered = decoded['unmasteredMix'][index]
        values = dict(unmasteredSum=abs(unmastered - voice - music),
                      voiceSharedGain=abs(mastered_voice - voice * gain),
                      musicSharedGain=abs(mastered_music - music * gain),
                      finalSum=abs(final[index] - mastered_voice - mastered_music),
                      finalSharedGain=abs(final[index] - unmastered * gain))
        for key, value in values.items():
            residuals[key] = max(residuals[key], value)
    float_tolerance = 1e-6 * max(1, gain)
    pcm_tolerance = 1 / 32768 + float_tolerance
    require(all(residuals[key] <= float_tolerance for key in ('unmasteredSum', 'voiceSharedGain', 'musicSharedGain')),
            f'Stems differ from the same mix graph/shared gain: {residuals}')
    require(all(residuals[key] <= pcm_tolerance for key in ('finalSum', 'finalSharedGain')),
            f'Final WAV differs from mastered stem sum: {residuals}')
    phrases = []
    for cue in timing['cues']:
        words = [word for word in timing['narration']['words'] if word['cueId'] == cue['id']]
        start_time, end_time = words[0]['finalStart'], words[-1]['finalEnd']
        start, end = round(start_time * RATE), round(end_time * RATE)
        require(0 <= start < end <= count, 'Active phrase outside stem sample extent')
        voice_rms = rms(decoded['voiceMastered'], start, end)
        music_rms = rms(decoded['musicMastered'], start, end)
        voice_db, music_db = decibels(voice_rms), decibels(music_rms)
        separation = voice_db - music_db if voice_db is not None and music_db is not None else None
        status = 'voice-silent' if voice_rms == 0 else ('music-silent' if music_rms == 0 else 'measured')
        passed = voice_rms > 0 and (music_rms == 0 or separation >= MINIMUM_SEPARATION_DB)
        phrases.append(dict(cueId=cue['id'], text=cue['text'], start=start_time, end=end_time,
                            startSample=start, endSample=end, sampleCount=end - start,
                            voiceRms=voice_rms, musicRms=music_rms, voiceDbfs=voice_db, musicDbfs=music_db,
                            separationDb=separation, status=status, passed=passed))
    measured = [phrase['separationDb'] for phrase in phrases if phrase['separationDb'] is not None]
    return dict(sampleRate=RATE, sampleCount=count, minimumRequiredDb=MINIMUM_SEPARATION_DB,
                minimumMeasuredDb=min(measured) if measured else None, phrases=phrases,
                masteringGainDb=mastering['gainDb'], maximumResiduals=residuals,
                floatTolerance=float_tolerance, pcmTolerance=pcm_tolerance,
                method='Actual same-graph post-duck stems; shared linear mastering; aligned whole-phrase RMS including internal pauses')


def enforce_balance(report):
    failures = [phrase for phrase in report['phrases'] if not phrase['passed']]
    require(not failures, 'Music/voice separation must be at least14dB in every active phrase: ' +
            '; '.join(f'cue {phrase["cueId"]}: {phrase["status"]}, separation={phrase["separationDb"]}' for phrase in failures))
