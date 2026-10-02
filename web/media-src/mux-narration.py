"""Mix the intact voice take and optional licensed music, copying reviewed video streams."""

import argparse
import json
import math
import shutil
import subprocess
from pathlib import Path

from media_contract import prepare_output, probe, read_timing, require, run, write_captions, write_json, write_manifest
from narration_proof import sha256, verify_words, verify_character_alignment
from soundtrack_proof import audio_record, measure_balance, enforce_balance


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


def mux(video_dir, audio, timing_path, alignment, captions_path, output_dir, proof_dir, music=None, music_metadata=None):
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
    credit = json.loads(music_metadata.read_text(encoding='utf-8-sig')) if music else None
    if credit:
        for key in ('title', 'author', 'sourcePage', 'license', 'licenseUrl', 'credit', 'sha256'):
            require(bool(credit.get(key)), f'Missing music provenance: {key}')
        require(sha256(music) == credit['sha256'], 'Music hash differs from license record')
    prepare_output(output_dir, [video_dir, audio, timing_path, alignment] + ([music] if music else []))
    prepare_output(proof_dir, [output_dir, video_dir, audio, timing_path])
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
        inputs += ['-stream_loop', '-1', '-i', music]
        graph = (f'[0:a]asplit=3[voice][key][voiceStem];[1:a]aresample=48000,aformat=channel_layouts=mono,'
                 f'atrim=duration={duration},loudnorm=I=-32:TP=-3:LRA=9,aresample=48000,'
                 f'afade=t=in:d=1,afade=t=out:st={duration - 3}:d=3[music];'
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
    write_json(proof_dir / 'soundtrack-balance.json', dict(mixGraph=graph, stems=stems, mastering=mastering, phraseBalance=balance))
    enforce_balance(balance)
    for kind, codec, bitrate in [('mp4', 'aac', '64k'), ('webm', 'libopus', '48k')]:
        video = video_dir / f'trailer.{kind}'
        require(abs(float(probe(video)['format']['duration']) - duration) <= .06, 'Reviewed picture duration mismatch')
        command = ['ffmpeg', '-v', 'error', '-y', '-i', video, '-i', final, '-map', '0:v:0', '-map', '1:a:0',
                   '-c:v', 'copy', '-c:a', codec, '-b:a', bitrate, '-ar', '48000', '-ac', '1',
                   '-map_metadata', '-1', '-metadata', 'title=Domios — a home for your agents']
        if credit:
            command += ['-metadata', f'comment={credit["credit"]}', '-metadata', f'copyright={credit["credit"]}']
        if kind == 'mp4':
            command += ['-movflags', '+faststart']
        run(command + [output_dir / video.name])
    for path in video_dir.glob('*-poster.webp'):
        shutil.copyfile(path, output_dir / path.name)
    write_captions(output_dir, captions, timing)
    manifest.update(hasNarration=True, subtitles=dict(src='/media/captions.vtt', bytes=(output_dir / 'captions.vtt').stat().st_size))
    for kind in ('mp4', 'webm'):
        manifest[kind]['bytes'] = (output_dir / f'trailer.{kind}').stat().st_size
    if credit:
        manifest['music'] = {key: credit[key] for key in ('title', 'author', 'sourcePage', 'license', 'licenseUrl', 'credit')}
        write_json(output_dir / 'credits.json', manifest['music'])
        (output_dir / 'credits.txt').write_text(credit['credit'] + '\n', encoding='utf-8', newline='\n')
    proof = dict(timingSha256=sha256(timing_path), sourceAudio=dict(path=str(audio.resolve()), sha256=sha256(audio)),
                 sourceAlignment=dict(path=str(alignment.resolve()), sha256=sha256(alignment)),
                 alignedVoice=dict(path=str(voice.resolve()), sha256=sha256(voice)),
                 finalAudio=dict(path=str(final.resolve()), sha256=sha256(final)),
                 sourceMusic=dict(path=str(music.resolve()), sha256=sha256(music)) if music else None,
                 narration=timing['narration'], music=credit, loudnessFirstPass=mastering['measurement'],
                 mixGraph=graph, mixStems=stems, mastering=mastering, phraseBalance=balance,
                 listening='Pending root full A/V audition and cue-boundary review')
    write_json(proof_dir / 'narration-alignment.json', proof)
    write_manifest(output_dir, manifest)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('video-dir', 'audio', 'timing', 'alignment', 'output-dir', 'proof-dir'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    for name in ('captions', 'music', 'music-metadata'):
        parser.add_argument(f'--{name}', type=Path)
    args = parser.parse_args()
    mux(args.video_dir, args.audio, args.timing, args.alignment, args.captions, args.output_dir,
        args.proof_dir, args.music, args.music_metadata)
