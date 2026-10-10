import math
import sys
from array import array
from pathlib import Path

from media_contract import require, run
from narration_proof import sha256


RATE = 48000
MINIMUM_SEPARATION_DB = 14
CONTINUITY_WINDOW_SAMPLES = RATE // 10
CONTINUITY_DROP_DBFS = -65
CONTINUITY_ONSET_DBFS = -45
CONTINUITY_AUDIBLE_END = round(44.8 * RATE)


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
    for key in ('unmasteredSum', 'voiceSharedGain', 'musicSharedGain'):
        require(residuals[key] <= float_tolerance, f'Stems do not share the same mix graph/shared gain: {key}')
    for key in ('finalSum', 'finalSharedGain'):
        require(residuals[key] <= pcm_tolerance, f'Final WAV differs from the measured mastered stem sum: {key}')
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


def verify_record(record, label):
    require(isinstance(record, dict) and isinstance(record.get('path'), str)
            and isinstance(record.get('sha256'), str) and len(record['sha256']) == 64,
            f'Missing hashed file record: {label}')
    path = Path(record['path'])
    require(sha256(path).lower() == record['sha256'].lower(), f'Hashed file changed: {label}')
    return path


def verify_exact_samples(expected, actual, label):
    require(len(expected) == len(actual), f'Sample count changed: {label}')
    require(all(first == second for first, second in zip(expected, actual)),
            f'Sample data changed: {label}')


def require_reconstruction_residual(residual, tolerance, label):
    require(math.isfinite(residual) and residual <= tolerance,
            f'Soundtrack reconstruction differs beyond tolerance: {label}={residual}')


def require_fixed_mastering(actual, prior):
    require(isinstance(prior, dict) and actual == prior,
            'Continuous mode must reuse the exact prior shared mastering record')
    gain = prior.get('linearGain')
    gain_db = prior.get('gainDb')
    require(isinstance(gain, (int, float)) and math.isfinite(gain) and gain > 0
            and isinstance(gain_db, (int, float)) and math.isfinite(gain_db)
            and abs(gain - 10 ** (gain_db / 20)) <= 1e-12,
            'Prior shared mastering gain is invalid')


def require_complete_phrase_balance(report, cue_ids):
    phrases = report.get('phrases')
    require(isinstance(phrases, list) and [phrase.get('cueId') for phrase in phrases] == cue_ids,
            'Phrase balance must include every aligned narration cue in order')
    require(all(phrase.get('passed') is True for phrase in phrases),
            'Every active phrase must meet the 14dB voice/music separation')


def measure_music_continuity(values, expected_count):
    require(len(values) == expected_count and expected_count % CONTINUITY_WINDOW_SAMPLES == 0,
            'Music continuity needs the complete programme in exact 100ms windows')
    windows = []
    for start in range(0, expected_count, CONTINUITY_WINDOW_SAMPLES):
        end = start + CONTINUITY_WINDOW_SAMPLES
        level = decibels(rms(values, start, end))
        windows.append(dict(startSample=start, endSample=end, startSeconds=start / RATE,
                            endSeconds=end / RATE, rmsDbfs=level))
    pre_fade = [window for window in windows if window['endSample'] <= CONTINUITY_AUDIBLE_END]
    final_fade = [window for window in windows if window['startSample'] >= CONTINUITY_AUDIBLE_END]
    return dict(sampleRate=RATE, sampleCount=expected_count, windowSamples=CONTINUITY_WINDOW_SAMPLES,
                windows=windows, onset=windows[0], dropoutCheckEndSample=CONTINUITY_AUDIBLE_END,
                preFadeWindows=len(pre_fade), finalFadeWindows=final_fade,
                dropoutFloorDbfs=CONTINUITY_DROP_DBFS, onsetFloorDbfs=CONTINUITY_ONSET_DBFS,
                finalFade=dict(startSample=CONTINUITY_AUDIBLE_END, endSample=expected_count,
                               durationSeconds=(expected_count - CONTINUITY_AUDIBLE_END) / RATE))


def enforce_music_continuity(report, label):
    windows = report['windows']
    onset = report['onset']['rmsDbfs']
    require(onset is not None and onset >= CONTINUITY_ONSET_DBFS,
            f'{label} first 100ms falls below -45dBFS')
    pre_fade = [window for window in windows if window['endSample'] <= CONTINUITY_AUDIBLE_END]
    for first, second in zip(pre_fade, pre_fade[1:]):
        first_level, second_level = first['rmsDbfs'], second['rmsDbfs']
        first_low = first_level is None or first_level < CONTINUITY_DROP_DBFS
        second_low = second_level is None or second_level < CONTINUITY_DROP_DBFS
        require(not (first_low and second_low),
                f'{label} has at least 200ms below -65dBFS before the final fade')
    require(report['finalFade']['durationSeconds'] == .2,
            f'{label} final authored fade must remain exactly 200ms')


def measure_phrase_gap_levels(timing, music):
    phrases = []
    for cue in timing['cues']:
        words = [word for word in timing['narration']['words'] if word['cueId'] == cue['id']]
        require(words, f'Cue {cue["id"]} has no aligned phrase for music level review')
        start = round(words[0]['finalStart'] * RATE)
        end = round(words[-1]['finalEnd'] * RATE)
        level = rms(music, start, end)
        phrases.append(dict(cueId=cue['id'], startSample=start, endSample=end,
                            musicRms=level, musicDbfs=decibels(level)))
    gaps = []
    first_start = phrases[0]['startSample']
    if first_start > 0:
        gaps.append(dict(id='opening', startSample=0, endSample=first_start))
    for left, right in zip(phrases, phrases[1:]):
        require(left['endSample'] <= right['startSample'], 'Aligned voice phrases overlap')
        if left['endSample'] < right['startSample']:
            gaps.append(dict(id=f'between-{left["cueId"]}-{right["cueId"]}',
                             startSample=left['endSample'], endSample=right['startSample']))
    final_start = phrases[-1]['endSample']
    if final_start < CONTINUITY_AUDIBLE_END:
        gaps.append(dict(id='final-hold', startSample=final_start, endSample=CONTINUITY_AUDIBLE_END))
    for gap in gaps:
        level = rms(music, gap['startSample'], gap['endSample'])
        gap['musicRms'] = level
        gap['musicDbfs'] = decibels(level)
    return dict(phrases=phrases, gaps=gaps, measuredStem='musicMastered')


def verify_continuous_arrangement(continuous_record, proof, timing, timing_path, mastering):
    from continuous_music import decode_source, read_json as read_continuous_json, render_music, source_record

    continuous_path = verify_record(continuous_record, 'continuous music proof')
    continuous = read_continuous_json(continuous_path)
    require(continuous.get('schema') == 'domios.continuous-music.proof.v1',
            'Unsupported continuous music proof')
    require(continuous.get('outputSampleCount') == round(timing['duration'] * RATE)
            and continuous.get('sampleRate') == RATE and timing['duration'] == 45,
            'Continuous proof does not cover the complete 45s soundtrack')
    require(continuous.get('baselineAudioProof', {}).get('sha256')
            == proof.get('baselineAudioProof', {}).get('sha256'),
            'Continuous proof and mux disagree on the prior audio authority')
    baseline_path = verify_record(continuous['baselineAudioProof'], 'prior narration proof')
    baseline = read_continuous_json(baseline_path)
    require(continuous.get('timingSha256') == sha256(timing_path),
            'Continuous proof timing authority changed')
    require_fixed_mastering(mastering, baseline['mastering'])
    require_fixed_mastering(continuous['fixedMastering'], baseline['mastering'])
    require(proof.get('music', {}).get('previousArrangement') == baseline.get('music', {}).get('arrangement'),
            'Previous Grok arrangement hash was not preserved')
    require(proof.get('music', {}).get('extractedAudioSha256', '').lower()
            == continuous.get('sourceSha256', '').lower(),
            'Published Grok provenance differs from the retained source')
    for key in ('title', 'author', 'sourcePage', 'license', 'licenseUrl', 'rightsStatus',
                'publishable', 'reviewOnly', 'originalMediaSha256', 'extractedAudioSha256',
                'publicationDecision'):
        require(proof['music'].get(key) == baseline['music'].get(key),
                f'Grok service provenance changed: {key}')
    metadata_path = verify_record(proof['musicProvenance'], 'continuous music provenance')
    require(read_continuous_json(metadata_path) == proof['music'],
            'Soundtrack proof differs from the hash-verified music provenance')
    require(proof['music'].get('arrangement') == continuous_record,
            'Music provenance does not bind the complete continuous arrangement proof')
    require(proof['music'].get('continuousProof') == continuous_record
            and proof['music'].get('editMap') == continuous.get('editMap')
            and proof['music'].get('source') == dict(path=continuous.get('source'),
                                                     sha256=continuous.get('sourceSha256')),
            'Grok source/edit lineage differs from the retained arrangement proof')
    source_path = verify_record(dict(path=continuous['source'], sha256=continuous['sourceSha256']),
                                'retained Grok source')
    scout_path = verify_record(continuous['scout'], 'source scout')
    scout = read_continuous_json(scout_path)
    source_record_value, silent_intervals = source_record(scout, source_path, continuous['sourceSha256'])
    require(continuous.get('sourceDurationSamples') > 0, 'Continuous proof lacks decoded source extent')
    map_path = verify_record(continuous['editMap'], 'source edit map')
    edit_map = read_continuous_json(map_path)
    require(edit_map.get('sourceSha256', '').lower() == continuous['sourceSha256'].lower(),
            'Edit map is bound to a different retained source')
    source_samples = decode_source(source_path)
    require(len(source_samples) == continuous['sourceDurationSamples'],
            'Retained source decoded sample count changed')
    require(len(source_samples) == round(float(source_record_value['duration']) * RATE),
            'Retained source extent differs from the scout measurement')
    expected_music, edit_audit = render_music(source_samples, timing, edit_map, silent_intervals)
    require(edit_audit == continuous.get('editAudit'), 'Continuous edit map audit differs')
    stems = proof.get('mixStems', {})
    required = {'voicePostDuck', 'musicPostDuck', 'voiceMastered', 'musicMastered', 'unmasteredMix'}
    require(set(stems) == required, 'Continuous proof lacks all actual mix stems')
    for key in required:
        verify_record(stems[key], f'continuous {key}')
    baseline_stems = baseline.get('mixStems', {})
    require(continuous.get('baselineStems') == {
        key: baseline_stems[key] for key in ('voicePostDuck', 'voiceMastered')},
        'Continuous proof does not retain the exact prior voice stem hashes')
    count = round(timing['duration'] * RATE)
    expected_voice_post = samples(baseline_stems['voicePostDuck'], count)
    actual_voice_post = samples(stems['voicePostDuck'], count)
    verify_exact_samples(expected_voice_post, actual_voice_post, 'voicePostDuck')
    expected_voice_mastered = samples(baseline_stems['voiceMastered'], count)
    actual_voice_mastered = samples(stems['voiceMastered'], count)
    verify_exact_samples(expected_voice_mastered, actual_voice_mastered, 'voiceMastered')
    prepared_record = continuous.get('preparedMusic')
    verify_record(prepared_record, 'prepared continuous music')
    require(proof.get('sourceMusic') == prepared_record,
            'Soundtrack music input differs from the arranged output record')
    require(prepared_record.get('sha256', '').lower() == stems['musicPostDuck']['sha256'].lower(),
            'Music post-duck stem differs from the arranged source output')
    actual_music = samples(stems['musicPostDuck'], count)
    verify_exact_samples(expected_music, actual_music, 'edit-map reconstructed music')
    prepared_levels = measure_music_continuity(actual_music, count)
    require(prepared_levels == continuous.get('continuity'),
            'Prepared music continuity envelope differs from its actual samples')
    enforce_music_continuity(prepared_levels, 'Prepared music')
    mastered_music = samples(stems['musicMastered'], count)
    mastered_levels = measure_music_continuity(mastered_music, count)
    phrase_gap_levels = measure_phrase_gap_levels(timing, mastered_music)
    require(phrase_gap_levels == proof.get('phraseGapMusic'),
            'Phrase/gap music levels differ from actual mastered samples')
    recorded = proof.get('musicContinuity')
    require(isinstance(recorded, dict) and recorded.get('prepared') == prepared_levels
            and recorded.get('mastered') == mastered_levels,
            'Prepared/mastered music continuity envelope differs from the actual stems')
    enforce_music_continuity(mastered_levels, 'Mastered music')
    return dict(sourceSha256=continuous['sourceSha256'], editMapSha256=continuous['editMap']['sha256'],
                preparedMusicSha256=prepared_record['sha256'],
                modificationDisclosure=proof['music']['modificationDisclosure'],
                segmentCount=len(edit_audit['segments']),
                joinCount=len(edit_audit['joins']), fixedMasteringGainDb=mastering['gainDb'],
                voicePostDuckPreserved=True, voiceMasteredPreserved=True,
                phraseGapMusic=phrase_gap_levels, editRampAudit=edit_audit,
                preparedMusicContinuity=prepared_levels, masteredMusicContinuity=mastered_levels)
