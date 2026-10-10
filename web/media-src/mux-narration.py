"""Mix intact voice and licensed or explicitly private prepared music, copying reviewed video."""

import argparse
import json
import math
import shutil
import subprocess
from array import array
from pathlib import Path

from media_contract import prepare_output, probe, read_timing, require, run, write_captions, write_json, write_manifest
from narration_proof import sha256, verify_words, verify_character_alignment
from soundtrack_proof import (audio_record, measure_balance, enforce_balance, measure_music_continuity,
                              enforce_music_continuity, verify_record, require_complete_phrase_balance,
                              measure_phrase_gap_levels)
from music_review import PRIVATE_MARKERS, validate_music, credit_payload, protect_private_path, prepared_music
from opening_audio import RATE, assemble_music, choose_gain, decode_excerpt, decode_samples
from opening_audio import (dbfs, rms, shape_excerpt, sum_samples, verify_preservation, write_float_wav,
                           write_pcm16_wav_preserving_tail, write_pcm16_wav)


def measure_loudness(source):
    measure = subprocess.run(['ffmpeg', '-hide_banner', '-i', str(source), '-vn',
                              '-af', 'loudnorm=I=-18:TP=-1.5:LRA=9:print_format=json', '-f', 'null', '-'],
                             check=True, capture_output=True, text=True, timeout=300)
    return json.JSONDecoder().raw_decode(measure.stderr[measure.stderr.rfind('{'):])[0]


def normalize(source, output):
    report = measure_loudness(source)
    values = ':'.join(f'{target}={report[source_key]}' for target, source_key in
                      [('measured_I', 'input_i'), ('measured_TP', 'input_tp'), ('measured_LRA', 'input_lra'),
                       ('measured_thresh', 'input_thresh'), ('offset', 'target_offset')])
    run(['ffmpeg', '-v', 'error', '-y', '-i', source, '-vn', '-af',
         f'loudnorm=I=-18:TP=-1.5:LRA=9:{values}:linear=true', '-ar', '48000', '-ac', '1', '-c:a', 'pcm_s16le', output])
    return report


def master_stems(soundtrack, voice_stem, music_stem, final, proof_dir):
    measurement = measure_loudness(soundtrack)
    level, peak = float(measurement['input_i']), float(measurement['input_tp'])
    require(math.isfinite(level) and math.isfinite(peak), 'No finite programme level for shared mastering')
    gain_db = min(-18 - level, -1.5 - peak)
    require(abs(level + gain_db + 18) <= 1, 'Shared linear gain cannot meet programme loudness/peak; root mix review required')
    voice_final, music_final = proof_dir / 'voice-final-stem.wav', proof_dir / 'music-final-stem.wav'
    graph = ';'.join(f'[{index}:a]volume={gain_db}dB:precision=double[{label}]'
                     for index, label in enumerate(('out', 'voice', 'music')))
    command = ['ffmpeg', '-v', 'error', '-y', '-i', soundtrack, '-i', voice_stem, '-i', music_stem,
               '-filter_complex', graph]
    for label, path, codec in [('out', final, 'pcm_s16le'), ('voice', voice_final, 'pcm_f32le'),
                               ('music', music_final, 'pcm_f32le')]:
        command += ['-map', f'[{label}]', '-ar', '48000', '-ac', '1', '-c:a', codec, path]
    run(command)
    stems = dict(voicePostDuck=audio_record(voice_stem), musicPostDuck=audio_record(music_stem),
                 voiceMastered=audio_record(voice_final), musicMastered=audio_record(music_final),
                 unmasteredMix=audio_record(soundtrack))
    mastering = dict(gainDb=gain_db, linearGain=10 ** (gain_db / 20), measurement=measurement,
                     method='Shared fixed linear gain on mix and both stems; no independent limiter or dynamic mastering')
    return stems, mastering


def verified_samples(record, count, label):
    path = Path(record['path'])
    require(sha256(path).lower() == record['sha256'].lower(), f'Prior audio proof changed: {label}')
    return decode_samples(path, count)


def mux_opening(video_dir, audio, timing_path, alignment, captions_path, output_dir, proof_dir, music,
                music_metadata, opening_source, baseline_audio_proof, opening_source_start, opening_end,
                private_review=False):
    timing = read_timing(timing_path)
    verify_words(timing)
    verify_character_alignment(timing, alignment)
    require(sha256(audio) == timing['narration']['audioSha256'], 'Input is not the intact approved voice recording')
    prior_path = Path(baseline_audio_proof)
    prior_hash = sha256(prior_path)
    prior = json.loads(prior_path.read_text(encoding='utf-8-sig'))
    require(prior.get('timingSha256') == sha256(timing_path), 'Prior audio proof uses different timing')
    require(prior.get('sourceAudio', {}).get('sha256') == sha256(audio), 'Prior proof does not use the supplied full voice take')
    require(prior.get('sourceAlignment', {}).get('sha256') == sha256(alignment), 'Prior proof does not use the supplied alignment')
    require(prior.get('narration') == timing['narration'], 'Prior proof has different word mapping')
    credit = json.loads(music_metadata.read_text(encoding='utf-8-sig'))
    credit = validate_music(credit, private_review)
    require(sha256(music).lower() == credit['sha256'].lower(), 'Arranged music hash differs from provenance')
    require(prior.get('music') == credit, 'Prior proof music provenance differs from supplied metadata')
    require(prior.get('sourceMusic', {}).get('sha256', '').lower() == credit['sha256'].lower(),
            'Prior proof does not reference the approved arranged music')
    require(sha256(Path(prior['sourceMusic']['path'])).lower() == prior['sourceMusic']['sha256'].lower(),
            'Prior arranged music source changed')
    extracted_hash = credit.get('extractedAudioSha256')
    require(isinstance(extracted_hash, str) and len(extracted_hash) == 64
            and sha256(opening_source).lower() == extracted_hash.lower(),
            'Opening source hash differs from credit.extractedAudioSha256')
    require(0 <= opening_source_start and .762 < opening_end <= 4,
            'Opening source selection must remain within the first four output seconds')

    manifest = json.loads((video_dir / 'manifest.json').read_text(encoding='utf-8-sig'))
    require(manifest['duration'] == timing['duration'] and manifest['chapters'] == timing['chapters'],
            'Picture differs from timing')
    require(manifest['timingSha256'] == sha256(timing_path), 'Picture uses different timing authority')
    if captions_path:
        captions = json.loads(captions_path.read_text(encoding='utf-8-sig'))
    else:
        captions = [{key: cue[key] for key in ('id', 'start', 'end', 'text')} for cue in timing['cues']]

    count = round(timing['duration'] * RATE)
    stems = prior.get('mixStems')
    require(isinstance(stems, dict) and set(stems) == {'voicePostDuck', 'musicPostDuck', 'voiceMastered',
                                                       'musicMastered', 'unmasteredMix'},
            'Prior proof lacks actual mastered and post-duck stems')
    for key in ('sourceAudio', 'sourceAlignment', 'alignedVoice'):
        record = prior[key]
        require(sha256(Path(record['path'])).lower() == record['sha256'].lower(),
                f'Prior audio proof changed: {key}')
    old_balance = measure_balance(timing, stems, prior['finalAudio'], prior['mastering'])
    require(old_balance == prior.get('phraseBalance'), 'Prior proof balance differs from its actual stem samples')
    enforce_balance(old_balance)
    prior_voice = verified_samples(stems['voiceMastered'], count, 'voiceMastered')
    prior_music = verified_samples(stems['musicMastered'], count, 'musicMastered')
    prior_voice_post_duck = verified_samples(stems['voicePostDuck'], count, 'voicePostDuck')
    excerpt_samples = round(opening_end * RATE)
    ramp_samples = round(.012 * RATE)
    crossfade_samples = round(.75 * RATE)
    crossfade_start = excerpt_samples - crossfade_samples
    excerpt = decode_excerpt(opening_source, opening_source_start, opening_end, excerpt_samples)
    excerpt_term = shape_excerpt(excerpt, crossfade_start, ramp_samples)
    gain, gain_report = choose_gain(prior_music, excerpt_term, prior_voice, timing, opening_end, count)
    new_music, _ = assemble_music(prior_music, excerpt, gain, excerpt_samples, crossfade_start, ramp_samples)
    require(dbfs(rms(new_music, 0, round(.1 * RATE))) > -50, 'First 100ms music RMS must exceed -50dBFS')
    require(dbfs(rms(new_music, 0, round(.65 * RATE))) > -40, 'First 650ms music RMS must exceed -40dBFS')
    mastering = prior['mastering']
    linear_gain = mastering['linearGain']
    require(math.isfinite(linear_gain) and linear_gain > 0, 'Prior shared mastering gain is invalid')
    new_music_post_duck = array('f', (value / linear_gain for value in new_music))
    unmastered = sum_samples(prior_voice_post_duck, new_music_post_duck)
    final_sum = sum_samples(prior_voice, new_music)
    prior_records = [Path(record['path']) for record in stems.values()]
    prior_records += [Path(prior['finalAudio']['path']), Path(prior['alignedVoice']['path'])]
    inputs = [video_dir, audio, timing_path, alignment, captions_path, music, music_metadata,
              opening_source, prior_path, *prior_records]
    protect_private_path(output_dir, inputs + [proof_dir])
    protect_private_path(proof_dir, inputs + [output_dir])
    for directory in (output_dir, proof_dir):
        require(not directory.exists() or not any(directory.iterdir()), 'Output/proof directory must be empty')
    prepare_output(output_dir, [path for path in inputs if path is not None])
    prepare_output(proof_dir, [output_dir] + [path for path in inputs if path is not None])

    voice_post_duck = proof_dir / 'voice-post-duck.wav'
    music_post_duck = proof_dir / 'music-post-duck.wav'
    voice_mastered = proof_dir / 'voice-final-stem.wav'
    music_mastered = proof_dir / 'music-final-stem.wav'
    unmastered_path = proof_dir / 'soundtrack-unmastered.wav'
    final = proof_dir / 'soundtrack.wav'
    shutil.copyfile(stems['voicePostDuck']['path'], voice_post_duck)
    write_float_wav(music_post_duck, new_music_post_duck)
    shutil.copyfile(stems['voiceMastered']['path'], voice_mastered)
    write_float_wav(music_mastered, new_music)
    copied_voice = decode_samples(voice_mastered, count)
    copied_music = decode_samples(music_mastered, count)
    verify_preservation(prior_voice, copied_voice, prior_music, copied_music, excerpt_samples)
    write_float_wav(unmastered_path, unmastered)
    write_pcm16_wav_preserving_tail(final, final_sum, prior['finalAudio']['path'], excerpt_samples)
    new_stems = dict(voicePostDuck=audio_record(voice_post_duck), musicPostDuck=audio_record(music_post_duck),
                     voiceMastered=audio_record(voice_mastered), musicMastered=audio_record(music_mastered),
                     unmasteredMix=audio_record(unmastered_path))
    final_record = audio_record(final)
    balance = measure_balance(timing, new_stems, final_record, mastering)
    enforce_balance(balance)
    audio_edit = dict(method='bounded opening excerpt overlay with cosine entry ramp and cosine join to the approved arrangement',
                      baselineAudioProof=dict(path=str(prior_path.resolve()), sha256=prior_hash),
                      baselineFinalAudio=prior['finalAudio'], baselineMasteredMusic=stems['musicMastered'],
                      baselineMasteredVoice=stems['voiceMastered'],
                      openingSource=dict(path=str(Path(opening_source).resolve()), sha256=sha256(opening_source),
                                         validatedAgainst='credit.extractedAudioSha256'),
                      arrangedMusic=dict(path=str(Path(music).resolve()), sha256=sha256(music),
                                         validatedAgainst='credit.sha256'),
                      sourceExcerpt=dict(startSeconds=opening_source_start, endSeconds=opening_source_start + opening_end,
                                         startSample=round(opening_source_start * RATE), sampleCount=excerpt_samples),
                      outputRange=dict(startSample=0, endSample=excerpt_samples,
                                       startSeconds=0, endSeconds=opening_end),
                      crossfade=dict(startSample=crossfade_start, endSample=excerpt_samples,
                                     startSeconds=crossfade_start / RATE, endSeconds=opening_end,
                                     durationSeconds=crossfade_samples / RATE),
                      antiClickRamp=dict(startSample=0, endSample=ramp_samples,
                                         durationSeconds=ramp_samples / RATE),
                      gain=gain_report, changedSampleRange=[0, excerpt_samples],
                      unchangedMusicSampleRange=[excerpt_samples, count],
                      finalAudioTailPreserved=True, finalAudioTailRange=[excerpt_samples, count],
                      voicePcmPreserved=True, postOpeningMusicPcmPreserved=True,
                      globalNormalization=False, reusedPriorMastering=True)
    exported_credit = credit_payload(credit, private_review) if credit else None
    markers = PRIVATE_MARKERS if private_review else {}
    write_json(proof_dir / 'soundtrack-balance.json', dict(
        mixGraph='baseline mastered voice + bounded source excerpt over baseline mastered music; no shared gain change',
        stems=new_stems, mastering=mastering, phraseBalance=balance, openingEdit=audio_edit, **markers))

    for kind, codec, bitrate in [('mp4', 'aac', '64k'), ('webm', 'libopus', '48k')]:
        video = video_dir / f'trailer.{kind}'
        require(abs(float(probe(video)['format']['duration']) - timing['duration']) <= .06,
                'Reviewed picture duration mismatch')
        command = ['ffmpeg', '-v', 'error', '-y', '-i', video, '-i', final, '-map', '0:v:0', '-map', '1:a:0',
                   '-c:v', 'copy', '-c:a', codec, '-b:a', bitrate, '-ar', str(RATE), '-ac', '1',
                   '-map_metadata', '-1', '-metadata', 'title=Domios — a home for your agents']
        if exported_credit:
            command += ['-metadata', f'comment={exported_credit["credit"]}',
                        '-metadata', f'copyright={exported_credit["credit"]}']
        if kind == 'mp4':
            command += ['-aac_coder', 'fast', '-movflags', '+faststart']
        run(command + [output_dir / video.name])
    for path in video_dir.glob('*-poster.webp'):
        shutil.copyfile(path, output_dir / path.name)
    write_captions(output_dir, captions, timing)
    manifest.update(hasNarration=True, subtitles=dict(src='/media/captions.vtt',
                                                      bytes=(output_dir / 'captions.vtt').stat().st_size))
    manifest.update(markers)
    for kind in ('mp4', 'webm'):
        manifest[kind]['bytes'] = (output_dir / f'trailer.{kind}').stat().st_size
    if exported_credit:
        manifest['music'] = exported_credit
        manifest['musicEditLineage'] = dict(
            openingSourceSha256=audio_edit['openingSource']['sha256'],
            arrangedMusicSha256=audio_edit['arrangedMusic']['sha256'],
            sourceExcerpt=audio_edit['sourceExcerpt'], outputRange=audio_edit['outputRange'],
            crossfade=audio_edit['crossfade'], antiClickRamp=audio_edit['antiClickRamp'],
            gain=audio_edit['gain'], changedSampleRange=audio_edit['changedSampleRange'])
        write_json(output_dir / 'credits.json', manifest['music'])
        (output_dir / 'credits.txt').write_text(exported_credit['credit'] + '\n', encoding='utf-8', newline='\n')
    proof = dict(timingSha256=sha256(timing_path), sourceAudio=prior['sourceAudio'],
                 sourceAlignment=prior['sourceAlignment'], alignedVoice=prior['alignedVoice'],
                 finalAudio=final_record, sourceMusic=prior['sourceMusic'], narration=timing['narration'],
                 music=credit, loudnessFirstPass=prior['loudnessFirstPass'],
                 mixGraph='baseline mastered voice + bounded source excerpt over baseline mastered music',
                 mixStems=new_stems, mastering=mastering, phraseBalance=balance,
                 musicInput=prior.get('musicInput'), openingEditLineage=audio_edit,
                 pictureReference=dict(directory=str(video_dir.resolve()), videos={
                     kind: dict(path=str((video_dir / f'trailer.{kind}').resolve()),
                                sha256=sha256(video_dir / f'trailer.{kind}')) for kind in ('mp4', 'webm')}),
                 baselineAudioProof=dict(path=str(prior_path.resolve()), sha256=prior_hash),
                 listening='Pending root full A/V audition and cue-boundary review', **markers)
    write_json(proof_dir / 'narration-alignment.json', proof)
    write_manifest(output_dir, manifest)
    return audio_edit


def mux_continuous(video_dir, audio, timing_path, alignment, captions_path, output_dir, proof_dir, music,
                   music_metadata, continuous_music_proof, private_review=False):
    timing = read_timing(timing_path)
    verify_words(timing)
    verify_character_alignment(timing, alignment)
    require(timing['duration'] == 45, 'Continuous soundtrack requires the complete 45s voiced film')
    require(sha256(audio) == timing['narration']['audioSha256'], 'Input is not the intact approved voice recording')
    manifest = json.loads((video_dir / 'manifest.json').read_text(encoding='utf-8-sig'))
    require(manifest['duration'] == timing['duration'] and manifest['chapters'] == timing['chapters'],
            'Picture differs from timing')
    require(manifest['timingSha256'] == sha256(timing_path),
            'Picture was rendered against a different timing authority')
    captions = json.loads(captions_path.read_text(encoding='utf-8-sig')) if captions_path else [
        {key: cue[key] for key in ('id', 'start', 'end', 'text')} for cue in timing['cues']]
    require(music is not None and music_metadata is not None and continuous_music_proof is not None,
            'Continuous mode requires arranged music, provenance, and its source/edit proof')
    require(not private_review, 'Continuous Grok mode preserves its existing publication terms')
    credit = validate_music(json.loads(music_metadata.read_text(encoding='utf-8-sig')), False)
    require(sha256(music).lower() == credit['sha256'].lower(), 'Arranged music hash differs from provenance')
    cp_path = verify_record(credit['continuousProof'], 'continuous music proof')
    cp = json.loads(cp_path.read_text(encoding='utf-8-sig'))
    cp_record = dict(path=str(cp_path.resolve()), sha256=sha256(cp_path))
    require(cp_record == dict(path=str(Path(continuous_music_proof).resolve()),
                              sha256=sha256(continuous_music_proof)),
            'Provenance and CLI name different continuous proofs')
    require(credit.get('arrangement') == cp_record and credit.get('continuousProof') == cp_record,
            'Grok provenance does not preserve the new arrangement proof hash')
    require(credit.get('source') == dict(path=cp.get('source'), sha256=cp.get('sourceSha256')),
            'Grok provenance source record differs from the edit proof')
    require(cp.get('outputSha256', '').lower() == sha256(music).lower()
            and cp.get('outputSampleCount') == round(timing['duration'] * RATE),
            'Prepared music differs from the complete source arrangement')
    require(credit.get('extractedAudioSha256', '').lower() == cp.get('sourceSha256', '').lower(),
            'Credit source hash differs from the retained Grok source')
    require(credit.get('editMap') == cp.get('editMap'), 'Music provenance and edit-map proof differ')
    require(credit.get('fixedMastering', {}).get('gainDb') == cp.get('fixedMastering', {}).get('gainDb')
            and credit.get('fixedMastering', {}).get('linearGain')
            == cp.get('fixedMastering', {}).get('linearGain'),
            'Provenance fixed gain differs from source proof')
    prior_path = verify_record(cp['baselineAudioProof'], 'prior narration proof')
    prior_hash = sha256(prior_path)
    prior = json.loads(prior_path.read_text(encoding='utf-8-sig'))
    require(prior.get('timingSha256') == sha256(timing_path)
            and prior.get('narration') == timing['narration'],
            'Prior mastered voice uses different timing or word mapping')
    require(prior.get('sourceAudio', {}).get('sha256') == sha256(audio)
            and prior.get('sourceAlignment', {}).get('sha256') == sha256(alignment),
            'Prior mastered voice uses a different narration take or alignment')
    require(prior.get('music', {}).get('extractedAudioSha256', '').lower() == cp['sourceSha256'].lower(),
            'Prior Grok provenance does not cover the retained source')
    old_credit = prior['music']
    for key in ('title', 'author', 'sourcePage', 'license', 'licenseUrl', 'rightsStatus',
                'publishable', 'reviewOnly', 'originalMediaSha256', 'extractedAudioSha256',
                'publicationDecision'):
        require(credit.get(key) == old_credit.get(key), f'Grok provenance changed: {key}')
    require(credit.get('previousArrangement') == old_credit.get('arrangement'),
            'Previous arrangement hash was not retained')
    prepared_info = prepared_music(music, timing['duration'])
    require(prepared_info['consumedSamples'] == round(timing['duration'] * RATE)
            and prepared_info['sourceSamples'] == round(timing['duration'] * RATE),
            'Continuous prepared music must contain exactly the complete programme')
    stems = prior.get('mixStems')
    required_stems = {'voicePostDuck', 'musicPostDuck', 'voiceMastered', 'musicMastered', 'unmasteredMix'}
    require(isinstance(stems, dict) and set(stems) == required_stems,
            'Prior proof lacks the actual voice stems and fixed gain')
    for key in ('voicePostDuck', 'voiceMastered'):
        verify_record(stems[key], f'Prior {key}')
    mastering = prior['mastering']
    require(mastering == cp.get('fixedMastering'), 'Prior fixed gain changed after source arrangement')
    prior_balance = measure_balance(timing, stems, prior['finalAudio'], mastering)
    require(prior_balance == prior.get('phraseBalance'), 'Prior mastered stems no longer reconstruct')
    enforce_balance(prior_balance)

    count = round(timing['duration'] * RATE)
    voice_post_duck = decode_samples(Path(stems['voicePostDuck']['path']), count)
    voice_mastered = decode_samples(Path(stems['voiceMastered']['path']), count)
    music_post_duck = decode_samples(music, count)
    gain = mastering['linearGain']
    music_mastered = array('f', (float(value) * gain for value in music_post_duck))
    unmastered = array('f', (float(voice) + float(music)
                             for voice, music in zip(voice_post_duck, music_post_duck)))
    final_samples = array('f', (float(voice) + float(music)
                               for voice, music in zip(voice_mastered, music_mastered)))
    require(all(math.isfinite(value) and abs(value) <= 1 for value in final_samples),
            'Fixed-gain continuous mix clips; candidate must stop for root review')
    prepared_levels = measure_music_continuity(music_post_duck, count)
    mastered_levels = measure_music_continuity(music_mastered, count)
    phrase_gap_levels = measure_phrase_gap_levels(timing, music_mastered)
    enforce_music_continuity(prepared_levels, 'Prepared music')
    enforce_music_continuity(mastered_levels, 'Mastered music')

    inputs = [video_dir, audio, timing_path, alignment, captions_path, music, music_metadata,
              continuous_music_proof, prior_path, Path(stems['voicePostDuck']['path']),
              Path(stems['voiceMastered']['path'])]
    protect_private_path(output_dir, inputs + [proof_dir])
    protect_private_path(proof_dir, inputs + [output_dir])
    for directory in (output_dir, proof_dir):
        require(not directory.exists() or not any(directory.iterdir()), 'Output/proof directory must be empty')
    prepare_output(output_dir, [path for path in inputs if path is not None])
    prepare_output(proof_dir, [output_dir] + [path for path in inputs if path is not None])

    voice_post_path = proof_dir / 'voice-post-duck.wav'
    music_post_path = proof_dir / 'music-post-duck.wav'
    voice_mastered_path = proof_dir / 'voice-final-stem.wav'
    music_mastered_path = proof_dir / 'music-final-stem.wav'
    unmastered_path = proof_dir / 'soundtrack-unmastered.wav'
    final = proof_dir / 'soundtrack.wav'
    shutil.copyfile(stems['voicePostDuck']['path'], voice_post_path)
    shutil.copyfile(music, music_post_path)
    shutil.copyfile(stems['voiceMastered']['path'], voice_mastered_path)
    write_float_wav(music_mastered_path, music_mastered)
    write_float_wav(unmastered_path, unmastered)
    write_pcm16_wav(final, final_samples)
    new_stems = dict(voicePostDuck=audio_record(voice_post_path), musicPostDuck=audio_record(music_post_path),
                     voiceMastered=audio_record(voice_mastered_path), musicMastered=audio_record(music_mastered_path),
                     unmasteredMix=audio_record(unmastered_path))
    mastering = dict(mastering)
    balance = measure_balance(timing, new_stems, audio_record(final), mastering)
    require_complete_phrase_balance(balance, [cue['id'] for cue in timing['cues']])
    enforce_balance(balance)
    exported_credit = credit_payload(credit, False)
    graph = ('hash-verified prior voicePostDuck/voiceMastered + edit-map continuous musicPostDuck; '
             'reuse prior fixed shared linear gain on music; quantize final PCM16 sum once')
    write_json(proof_dir / 'soundtrack-balance.json', dict(
        mixGraph=graph, stems=new_stems, mastering=mastering, phraseBalance=balance,
        musicContinuity=dict(prepared=prepared_levels, mastered=mastered_levels),
        phraseGapMusic=phrase_gap_levels,
        continuousMusicProof=cp_record, baselineAudioProof=dict(path=str(prior_path.resolve()), sha256=prior_hash)))

    for kind, codec, bitrate in [('mp4', 'aac', '64k'), ('webm', 'libopus', '48k')]:
        video = video_dir / f'trailer.{kind}'
        require(abs(float(probe(video)['format']['duration']) - timing['duration']) <= .06,
                'Reviewed picture duration mismatch')
        command = ['ffmpeg', '-v', 'error', '-y', '-i', video, '-i', final,
                   '-map', '0:v:0', '-map', '1:a:0', '-c:v', 'copy', '-c:a', codec,
                   '-b:a', bitrate, '-ar', str(RATE), '-ac', '1', '-map_metadata', '-1',
                   '-metadata', 'title=Domios — a home for your agents',
                   '-metadata', f'comment={exported_credit["credit"]}',
                   '-metadata', f'copyright={exported_credit["credit"]}']
        if kind == 'mp4':
            command += ['-aac_coder', 'fast', '-movflags', '+faststart']
        else:
            command += ['-frame_duration', '10']
        run(command + [output_dir / video.name])
    for path in video_dir.glob('*-poster.webp'):
        shutil.copyfile(path, output_dir / path.name)
    write_captions(output_dir, captions, timing)
    manifest.update(hasNarration=True,
                    subtitles=dict(src='/media/captions.vtt', bytes=(output_dir / 'captions.vtt').stat().st_size))
    for kind in ('mp4', 'webm'):
        manifest[kind]['bytes'] = (output_dir / f'trailer.{kind}').stat().st_size
    manifest['music'] = exported_credit
    manifest['musicArrangement'] = dict(sourceSha256=cp['sourceSha256'], editMapSha256=cp['editMap']['sha256'],
                                        arrangementSha256=sha256(music),
                                        modificationDisclosure=credit['modificationDisclosure'])
    write_json(output_dir / 'credits.json', manifest['music'])
    (output_dir / 'credits.txt').write_text(exported_credit['credit'] + '\n', encoding='utf-8', newline='\n')
    continuous_record = dict(path=str(Path(continuous_music_proof).resolve()), sha256=sha256(continuous_music_proof))
    baseline_record = dict(path=str(prior_path.resolve()), sha256=prior_hash)
    proof = dict(timingSha256=sha256(timing_path),
                 sourceAudio=dict(path=str(audio.resolve()), sha256=sha256(audio)),
                 sourceAlignment=dict(path=str(alignment.resolve()), sha256=sha256(alignment)),
                 alignedVoice=prior['alignedVoice'], finalAudio=audio_record(final),
                 sourceMusic=dict(path=str(music.resolve()), sha256=sha256(music)),
                 narration=timing['narration'], music=credit, loudnessFirstPass=prior['loudnessFirstPass'],
                 mixGraph=graph, mixStems=new_stems, mastering=mastering, phraseBalance=balance,
                 musicInput=prepared_info, musicProvenance=dict(path=str(Path(music_metadata).resolve()),
                                                               sha256=sha256(music_metadata)),
                 continuousMusicProof=continuous_record,
                 baselineAudioProof=baseline_record,
                 musicContinuity=dict(prepared=prepared_levels, mastered=mastered_levels),
                 phraseGapMusic=phrase_gap_levels,
                 continuousMusic=dict(sourceSha256=cp['sourceSha256'], editMapSha256=cp['editMap']['sha256'],
                                      arrangementSha256=sha256(music), fixedGainReused=True,
                                      finalFade=prepared_levels['finalFade']),
                 pictureReference=dict(directory=str(video_dir.resolve()), videos={
                     kind: dict(path=str((video_dir / f'trailer.{kind}').resolve()),
                                sha256=sha256(video_dir / f'trailer.{kind}')) for kind in ('mp4', 'webm')}),
                 listening='Pending root full A/V audition, every edit join, phrase ducking, and final hold')
    write_json(proof_dir / 'narration-alignment.json', proof)
    write_manifest(output_dir, manifest)
    return dict(continuousProof=continuous_record, baselineAudioProof=baseline_record,
                phraseBalance=balance, musicContinuity=proof['musicContinuity'])


def mux(video_dir, audio, timing_path, alignment, captions_path, output_dir, proof_dir, music=None, music_metadata=None,
        music_prepared=False, private_review=False, opening_source=None, baseline_audio_proof=None,
        opening_source_start=2.6, opening_end=4, continuous_music_proof=None, opus_frame_duration=20):
    require(opus_frame_duration in (10, 20), 'Unsupported Opus frame duration')
    if continuous_music_proof is not None:
        require(opening_source is None and baseline_audio_proof is None and music is not None
                and music_metadata is not None and music_prepared,
                'Continuous mode requires only the proved prepared music and its provenance')
        return mux_continuous(video_dir, audio, timing_path, alignment, captions_path, output_dir, proof_dir,
                              music, music_metadata, continuous_music_proof, private_review)
    if opening_source is not None or baseline_audio_proof is not None:
        require(opening_source is not None and baseline_audio_proof is not None and music is not None and music_metadata is not None,
                'Opening mode requires --music, --music-metadata, --opening-source, and --baseline-audio-proof')
        return mux_opening(video_dir, audio, timing_path, alignment, captions_path, output_dir, proof_dir, music,
                           music_metadata, opening_source, baseline_audio_proof, opening_source_start, opening_end,
                           private_review)
    timing = read_timing(timing_path)
    verify_words(timing)
    verify_character_alignment(timing, alignment)
    require(sha256(audio) == timing['narration']['audioSha256'], 'Input is not the intact approved voice recording')
    manifest = json.loads((video_dir / 'manifest.json').read_text(encoding='utf-8-sig'))
    require(manifest['duration'] == timing['duration'] and manifest['chapters'] == timing['chapters'], 'Picture differs from timing')
    require(manifest['timingSha256'] == sha256(timing_path), 'Picture was rendered against a different timing authority')
    if captions_path:
        captions = json.loads(captions_path.read_text(encoding='utf-8-sig'))
    else:
        captions = [{key: cue[key] for key in ('id', 'start', 'end', 'text')} for cue in timing['cues']]
    require(not music or music_metadata, 'Music needs root-reviewed license metadata')
    require((music is None) == (music_metadata is None), 'Provide music and metadata together')
    require(not music_prepared or music is not None, 'Prepared mode requires music input')
    require(not private_review or (music is not None and music_prepared), 'Private review requires prepared music/provenance')
    credit = json.loads(music_metadata.read_text(encoding='utf-8-sig')) if music else None
    require(not music or isinstance(credit, dict), 'Music provenance must be a metadata object')
    if credit is not None:
        credit = validate_music(credit, private_review)
        require(sha256(music).lower() == credit['sha256'].lower(), 'Music hash differs from provenance record')
    music_input = prepared_music(music, timing['duration']) if music_prepared else None
    inputs = [video_dir, audio, timing_path, alignment, captions_path, music, music_metadata]
    if private_review:
        inputs.append(Path(credit['arrangement']['path']))
        protect_private_path(output_dir, inputs + [proof_dir])
        protect_private_path(proof_dir, inputs + [output_dir])
    for directory in (output_dir, proof_dir):
        require(not directory.exists() or not any(directory.iterdir()), 'Output/proof directory must be empty')
    prepare_output(output_dir, [path for path in inputs if path is not None])
    prepare_output(proof_dir, [output_dir] + [path for path in inputs if path is not None])
    voice = proof_dir / 'narration-aligned.wav'
    offset = timing['narration']['offset']
    duration = timing['duration']
    filters = f'aresample=48000,aformat=channel_layouts=mono,adelay={round(offset * 48000)}S:all=1,apad,atrim=duration={duration}'
    run(['ffmpeg', '-v', 'error', '-y', '-i', audio, '-vn', '-af', filters, '-ar', '48000', '-ac', '1', voice])
    normalized_voice = proof_dir / 'narration-mastered.wav'
    normalize(voice, normalized_voice)
    soundtrack = proof_dir / 'soundtrack-unmastered.wav'
    voice_stem, music_stem = proof_dir / 'voice-post-duck.wav', proof_dir / 'music-post-duck.wav'
    inputs = ['ffmpeg', '-v', 'error', '-y', '-i', normalized_voice]
    if music:
        inputs += (['-i', music] if music_prepared else ['-stream_loop', '-1', '-i', music])
        fades = '' if music_prepared else f',afade=t=in:d=1,afade=t=out:st={duration - 3}:d=3'
        graph = (f'[0:a]asplit=3[voice][key][voiceStem];[1:a]aresample=48000,aformat=channel_layouts=mono,'
                 f'atrim=duration={duration},loudnorm=I=-32:TP=-3:LRA=9,aresample=48000{fades}[music];'
                 '[music][key]sidechaincompress=threshold=0.018:ratio=8:attack=15:release=450[postDuck];'
                 '[postDuck]asplit=2[ducked][musicStem];'
                 '[voice][ducked]amix=inputs=2:duration=first:normalize=0[out]')
    else:
        graph = '[0:a]asplit=3[out][voiceStem][silent];[silent]volume=0[musicStem]'
    command = inputs + ['-filter_complex', graph]
    for label, path in [('out', soundtrack), ('voiceStem', voice_stem), ('musicStem', music_stem)]:
        command += ['-map', f'[{label}]', '-ar', '48000', '-ac', '1', '-c:a', 'pcm_f32le', path]
    run(command)
    final = proof_dir / 'soundtrack.wav'
    stems, mastering = master_stems(soundtrack, voice_stem, music_stem, final, proof_dir)
    balance = measure_balance(timing, stems, audio_record(final), mastering)
    markers = PRIVATE_MARKERS if private_review else {}
    write_json(proof_dir / 'soundtrack-balance.json', dict(mixGraph=graph, stems=stems, mastering=mastering,
                                                        phraseBalance=balance, **markers))
    enforce_balance(balance)
    exported_credit = credit_payload(credit, private_review) if credit else None
    for kind, codec, bitrate in [('mp4', 'aac', '64k'), ('webm', 'libopus', '48k')]:
        video = video_dir / f'trailer.{kind}'
        require(abs(float(probe(video)['format']['duration']) - duration) <= .06, 'Reviewed picture duration mismatch')
        command = ['ffmpeg', '-v', 'error', '-y', '-i', video, '-i', final, '-map', '0:v:0', '-map', '1:a:0',
                   '-c:v', 'copy', '-c:a', codec, '-b:a', bitrate, '-ar', '48000', '-ac', '1',
                   '-map_metadata', '-1', '-metadata', 'title=Domios — a home for your agents']
        if credit:
            command += ['-metadata', f'comment={exported_credit["credit"]}', '-metadata', f'copyright={exported_credit["credit"]}']
        if kind == 'mp4':
            command += ['-aac_coder', 'fast', '-movflags', '+faststart']
        else:
            command += ['-frame_duration', str(opus_frame_duration)]
        run(command + [output_dir / video.name])
    for path in video_dir.glob('*-poster.webp'):
        shutil.copyfile(path, output_dir / path.name)
    write_captions(output_dir, captions, timing)
    manifest.update(hasNarration=True, subtitles=dict(src='/media/captions.vtt', bytes=(output_dir / 'captions.vtt').stat().st_size))
    manifest.update(markers)
    for kind in ('mp4', 'webm'):
        manifest[kind]['bytes'] = (output_dir / f'trailer.{kind}').stat().st_size
    if credit:
        manifest['music'] = exported_credit
        write_json(output_dir / 'credits.json', manifest['music'])
        (output_dir / 'credits.txt').write_text(exported_credit['credit'] + '\n', encoding='utf-8', newline='\n')
    proof = dict(timingSha256=sha256(timing_path), sourceAudio=dict(path=str(audio.resolve()), sha256=sha256(audio)),
                 sourceAlignment=dict(path=str(alignment.resolve()), sha256=sha256(alignment)),
                 alignedVoice=dict(path=str(voice.resolve()), sha256=sha256(voice)),
                 finalAudio=dict(path=str(final.resolve()), sha256=sha256(final)),
                 sourceMusic=dict(path=str(music.resolve()), sha256=sha256(music)) if music else None,
                 narration=timing['narration'], music=credit, loudnessFirstPass=mastering['measurement'],
                 mixGraph=graph, mixStems=stems, mastering=mastering, phraseBalance=balance,
                 musicInput=music_input, audioEncoding=dict(opusFrameDurationMs=opus_frame_duration),
                 **markers,
                 listening='Pending root full A/V audition and cue-boundary review')
    write_json(proof_dir / 'narration-alignment.json', proof)
    write_manifest(output_dir, manifest)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('video-dir', 'audio', 'timing', 'alignment', 'output-dir', 'proof-dir'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    for name in ('captions', 'music', 'music-metadata'):
        parser.add_argument(f'--{name}', type=Path)
    parser.add_argument('--music-prepared', action='store_true')
    parser.add_argument('--private-review', action='store_true')
    parser.add_argument('--opening-source', type=Path)
    parser.add_argument('--baseline-audio-proof', type=Path)
    parser.add_argument('--opening-source-start', type=float, default=2.6)
    parser.add_argument('--opening-end', type=float, default=4)
    parser.add_argument('--continuous-music-proof', type=Path)
    parser.add_argument('--opus-frame-duration', type=int, choices=(10, 20), default=20)
    args = parser.parse_args()
    mux(args.video_dir, args.audio, args.timing, args.alignment, args.captions, args.output_dir,
        args.proof_dir, args.music, args.music_metadata, args.music_prepared, args.private_review,
        args.opening_source, args.baseline_audio_proof, args.opening_source_start, args.opening_end,
        args.continuous_music_proof, args.opus_frame_duration)
