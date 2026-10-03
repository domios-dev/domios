"""Canonical media oracle; run only under the integration owner's authority."""

import argparse
import json
import struct
import subprocess
from fractions import Fraction
from pathlib import Path

from PIL import Image

from media_contract import probe, read_timing, require, run, validate_captions, write_json
from narration_proof import sha256, verify_words, verify_character_alignment
from timeline_audit import timeline_audit
from soundtrack_proof import (measure_balance, enforce_balance, require_complete_phrase_balance,
                              verify_continuous_arrangement)
from music_review import (PRIVATE_MARKERS, validate_markers, validate_music, validate_license,
                          credit_payload, protect_private_path, prepared_music)


def preserved_baseline(directory, manifest):
    records = json.loads(manifest.read_text(encoding='utf-8-sig'))
    expected = set()
    for record in records:
        path = Path(record['Path'])
        require(path.resolve().parent == directory.resolve(), 'Baseline hash record outside preserved directory')
        require(sha256(path).upper() == record['Hash'].upper(), f'Baseline changed: {path}')
        expected.add(path.name)
    require(expected == {path.name for path in directory.iterdir() if path.is_file()}, 'Incomplete baseline inventory')
    reference = run(['git', 'rev-parse', 'refs/heads/archive/domios-trailer-before-narration'], timeout=30, capture=True).decode().strip()
    require(reference == '222f60c46b0f962d566344c99b96b5da3afade7f', 'Preserved archive branch moved')
    for kind in ('mp4', 'webm'):
        require(all(stream['codec_type'] != 'audio' for stream in probe(directory / f'trailer.{kind}')['streams']), 'Original has audio')
    return dict(files=len(expected), archive=reference, unchanged=True)


def faststart(path):
    atoms = {}
    with path.open('rb') as video:
        while header := video.read(8):
            require(len(header) == 8, 'Invalid MP4 atom')
            size, kind = struct.unpack('>I4s', header)
            position = video.tell() - 8
            if size == 1:
                size = struct.unpack('>Q', video.read(8))[0]
            if size == 0:
                break
            require(size >= 8, 'Invalid MP4 atom size')
            atoms[kind] = position
            video.seek(position + size)
    require(b'moov' in atoms and b'mdat' in atoms and atoms[b'moov'] < atoms[b'mdat'], 'MP4 lacks faststart')


def frame_hashes(path):
    output = run(['ffmpeg', '-v', 'error', '-i', path, '-map', '0:v:0', '-an', '-fps_mode', 'passthrough', '-f', 'framemd5', '-'], capture=True)
    return [line.rsplit(',', 1)[-1].strip() for line in output.decode().splitlines() if line and not line.startswith('#')]


def audio_packet_oracle(candidate, mixed_audio, continuous_music=False, opus_frame_duration=None):
    require(opus_frame_duration in (None, 10, 20), 'Unsupported declared Opus frame duration')
    actual = run(['ffmpeg', '-v', 'error', '-i', candidate, '-map', '0:a:0', '-c:a', 'copy',
                  '-f', 'streamhash', '-hash', 'sha256', '-'], capture=True).decode().strip()
    codec, bitrate = ('aac', '64k') if candidate.suffix == '.mp4' else ('libopus', '48k')
    reference = ['ffmpeg', '-v', 'error', '-i', mixed_audio, '-map', '0:a:0', '-c:a', codec,
                 '-b:a', bitrate, '-ar', '48000', '-ac', '1']
    if candidate.suffix == '.mp4':
        reference += ['-aac_coder', 'fast']
    elif opus_frame_duration is not None or continuous_music:
        reference += ['-frame_duration', str(opus_frame_duration if opus_frame_duration is not None else 10)]
    expected = run(reference + ['-f', 'streamhash', '-hash', 'sha256', '-'], capture=True).decode().strip()
    require(actual == expected, 'Encoded soundtrack differs from the aligned/mixed proof WAV')
    return actual


def loudness(path):
    result = subprocess.run(['ffmpeg', '-hide_banner', '-i', str(path), '-vn', '-af',
                             'loudnorm=I=-18:TP=-1:LRA=9:print_format=json', '-f', 'null', '-'],
                            check=True, timeout=300, capture_output=True, text=True)
    report = json.JSONDecoder().raw_decode(result.stderr[result.stderr.rfind('{'):])[0]
    require(abs(float(report['input_i']) + 18) <= 1, 'Programme loudness outside-18LUFS +/-1')
    require(float(report['input_tp']) <= -1, 'True peak exceeds-1dBTP')
    return report


def video_oracle(path, timing, require_audio):
    info = probe(path)
    streams = info['streams']
    videos = [stream for stream in streams if stream['codec_type'] == 'video']
    audios = [stream for stream in streams if stream['codec_type'] == 'audio']
    require(len(videos) == 1 and len(audios) == int(require_audio) and len(streams) == 1 + int(require_audio), 'Wrong stream inventory')
    video = videos[0]
    kind = path.suffix
    require(video['codec_name'] == ('h264' if kind == '.mp4' else 'vp9'), 'Wrong picture codec')
    require((video['width'], video['height'], video['pix_fmt']) == (1280, 720, 'yuv420p'), 'Wrong picture dimensions/format')
    require(video['avg_frame_rate'] == '30/1' and video['r_frame_rate'] == '30/1', 'Picture is not CFR30')
    frame_count = round(timing['duration'] * 30)
    require(int(video['nb_read_frames']) == frame_count, 'Decoded frame count differs from chosen duration')
    require(abs(float(info['format']['duration']) - timing['duration']) <= .06, 'Container duration mismatch')
    frames = json.loads(run(['ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_frames',
                             '-show_entries', 'frame=best_effort_timestamp_time', '-of', 'json', path],
                            timeout=120, capture=True))['frames']
    pts = [float(frame['best_effort_timestamp_time']) for frame in frames]
    require(len(pts) == frame_count and all(a < b for a, b in zip(pts, pts[1:])), 'Invalid video PTS')
    tick = float(Fraction(video['time_base']))
    tolerance = tick + 1e-6 if kind == '.webm' else (1e-5 if Fraction(video['time_base']).denominator % 30 == 0 else max(1e-5, tick / 2 + 1e-6))
    require(abs(pts[0]) < 1 / 30, 'Picture has more than one frame of initial container offset')
    residual = max(abs(value - pts[0] - index / 30) for index, value in enumerate(pts))
    require(residual <= tolerance, 'Frame clock differs from CFR30')
    if require_audio:
        audio = audios[0]
        require(audio['codec_name'] == ('aac' if kind == '.mp4' else 'opus'), 'Wrong audio codec')
        require(audio['channels'] == 1 and audio['sample_rate'] == '48000', 'Expected mono48kHz')
    run(['ffmpeg', '-v', 'error', '-xerror', '-i', path, '-f', 'null', '-'])
    if kind == '.mp4':
        faststart(path)
    return dict(frames=frame_count, duration=info['format']['duration'], timeBase=video['time_base'], firstPTS=pts[0],
                maximumResidual=residual, ptsTolerance=tolerance, loudness=loudness(path) if require_audio else None)


def published_assets(directory, timing, require_audio, timing_path, private_review=False):
    manifest = json.loads((directory / 'manifest.json').read_text(encoding='utf-8-sig'))
    validate_markers(manifest, private_review)
    require(manifest['duration'] == timing['duration'] and manifest['chapters'] == timing['chapters'], 'Manifest differs from timing authority')
    require(manifest.get('timingSha256') == (sha256(timing_path) if timing_path else None), 'Rendered timing authority changed')
    require((manifest['width'], manifest['height'], manifest['fps']) == (1280, 720, 30), 'Manifest picture mismatch')
    require(bool(manifest.get('hasNarration')) == require_audio, 'Sound-control metadata differs from streams')
    captions = json.loads((directory / 'captions.json').read_text(encoding='utf-8-sig'))
    validate_captions(captions, timing)
    require(json.loads((directory / 'transcript.json').read_text(encoding='utf-8-sig')) == captions, 'Transcript differs from captions')
    sections = (directory / 'captions.vtt').read_text(encoding='utf-8-sig').strip().split('\n\n')
    require(sections[0] == 'WEBVTT' and len(sections) == len(captions) + 1, 'Invalid VTT cue count')

    def seconds(value):
        hours, minutes, seconds = map(float, value.split(':'))
        return hours * 3600 + minutes * 60 + seconds

    for section, cue in zip(sections[1:], captions):
        _, times, text = section.split('\n', 2)
        start, end = map(seconds, times.split(' --> '))
        require(abs(start - cue['start']) <= .000501 and abs(end - cue['end']) <= .000501 and text == cue['text'], 'VTT differs from speech')
    allowed = {'trailer.mp4', 'trailer.webm', 'trailer-poster.webp', 'manifest.json', 'captions.json',
               'captions.vtt', 'transcript.json'} | {f'{chapter["id"]}-poster.webp' for chapter in timing['chapters']}
    if manifest.get('music'):
        if private_review:
            validate_music(manifest['music'], True)
        else:
            validate_license(manifest['music'])
        allowed |= {'credits.json', 'credits.txt'}
        require(json.loads((directory / 'credits.json').read_text(encoding='utf-8-sig')) == manifest['music'], 'Music credits differ')
        require((directory / 'credits.txt').read_text(encoding='utf-8-sig').strip() == manifest['music']['credit'], 'Music attribution missing')
        arrangement = manifest.get('musicArrangement')
        if arrangement is not None:
            require(all(isinstance(arrangement.get(key), str) and arrangement[key]
                        for key in ('sourceSha256', 'editMapSha256', 'arrangementSha256',
                                    'modificationDisclosure')),
                    'Continuous arrangement disclosure is incomplete')
            require(all(len(arrangement[key]) == 64 for key in
                        ('sourceSha256', 'editMapSha256', 'arrangementSha256')),
                    'Continuous arrangement hashes are invalid')
    require({path.name for path in directory.iterdir()} == allowed, 'Unexpected/missing published files; no private masters allowed')
    assets = [manifest[key] for key in ('mp4', 'webm', 'poster')]
    if require_audio:
        assets.append(manifest['subtitles'])
    for asset in assets:
        require(asset['src'].startswith('/media/') and '..' not in asset['src'], 'Invalid asset URL')
        path = directory / asset['src'].removeprefix('/media/')
        require(path.is_file() and path.stat().st_size == asset['bytes'], 'Asset byte count mismatch')
    for name in ('trailer', 'agents', 'board', 'mesh', 'plugins'):
        with Image.open(directory / f'{name}-poster.webp') as poster:
            require(poster.size == ((1280, 720) if name == 'trailer' else (480, 270)), 'Poster size mismatch')
            poster.load()
    total = sum(path.stat().st_size for path in directory.iterdir())
    require(total == manifest['totalBytes'], 'Aggregate manifest size mismatch')
    return dict(totalBytes=total, files=len(allowed), music=manifest.get('music'),
                musicArrangement=manifest.get('musicArrangement'))


def narration_oracle(proof_path, timing, timing_path, private_review=False):
    proof = json.loads(proof_path.read_text(encoding='utf-8-sig'))
    validate_markers(proof, private_review)
    require(proof['timingSha256'] == sha256(timing_path), 'Timing changed after audio mix')
    require(proof['narration'] == timing['narration'], 'Word mapping differs from timing')
    verify_words(timing)
    for key in ('sourceAudio', 'sourceAlignment', 'alignedVoice', 'finalAudio'):
        record = proof[key]
        require(sha256(Path(record['path'])) == record['sha256'], f'Audio provenance changed: {key}')
    require(proof['sourceAudio']['sha256'] == timing['narration']['audioSha256'], 'Wrong actual voice take')
    verify_character_alignment(timing, Path(proof['sourceAlignment']['path']))
    if proof['music']:
        validate_music(proof['music'], private_review)
        music = proof['sourceMusic']
        require(sha256(Path(music['path'])).lower() == music['sha256'].lower() == proof['music']['sha256'].lower(),
                'Music source/license provenance changed')
        if proof.get('musicInput'):
            require(prepared_music(Path(music['path']), timing['duration']) == proof['musicInput'], 'Prepared input proof changed')
    if private_review:
        require(proof['music'] and isinstance(proof.get('musicInput'), dict) and proof['musicInput'].get('prepared') is True,
                'Private review requires prepared music provenance')
    for key in ('alignedVoice', 'finalAudio'):
        info = probe(Path(proof[key]['path']))
        require(abs(float(info['format']['duration']) - timing['duration']) <= 1 / 48000, 'PCM duration differs from timing')
    balance = measure_balance(timing, proof['mixStems'], proof['finalAudio'], proof['mastering'])
    require(balance == proof['phraseBalance'], 'Recorded phrase levels differ from actual hashed stem samples')
    enforce_balance(balance)
    continuous = None
    if proof.get('continuousMusicProof'):
        require_complete_phrase_balance(balance, [cue['id'] for cue in timing['cues']])
        continuous = verify_continuous_arrangement(proof['continuousMusicProof'], proof, timing, timing_path,
                                                   proof['mastering'])
    return dict(words=len(timing['narration']['words']), sourceAudioSha256=proof['sourceAudio']['sha256'],
                finalAudioPath=proof['finalAudio']['path'], finalAudioSha256=proof['finalAudio']['sha256'],
                phraseBalance=balance,
                continuousMusic=continuous, audioEncoding=proof.get('audioEncoding', {}),
                musicCredit=credit_payload(proof['music'], private_review) if proof['music'] else None,
                listening='Root must audition complete A/V and every cue; metadata cannot prove pronunciation or clicks')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--timeline-only', action='store_true')
    parser.add_argument('--baseline-ref', default='222f60c46')
    parser.add_argument('--require-audio', action='store_true')
    parser.add_argument('--private-review', action='store_true')
    for name in ('baseline-dir', 'baseline-manifest', 'candidate-dir', 'reference-video-dir', 'timing', 'narration-alignment', 'audit'):
        parser.add_argument(f'--{name}', type=Path)
    args = parser.parse_args()
    if args.private_review:
        require(not args.timeline_only and args.require_audio and args.candidate_dir and args.narration_alignment,
                'Private review requires the complete audio/media oracle')
        inputs = [args.baseline_dir, args.baseline_manifest, args.reference_video_dir, args.timing, args.narration_alignment]
        protect_private_path(args.candidate_dir, inputs)
        protect_private_path(args.narration_alignment, [args.baseline_dir, args.reference_video_dir])
        if args.audit:
            protect_private_path(args.audit, inputs + [args.candidate_dir])
    if args.audit:
        for directory in (args.baseline_dir, args.candidate_dir, Path(__file__).resolve().parents[1] / 'public/media'):
            if directory:
                require(directory.resolve() not in args.audit.resolve().parents, 'Audit must remain outside preserved/published media')
    timing = read_timing(args.timing)
    report = dict(timeline=timeline_audit(args.baseline_ref, timing=timing), **(PRIVATE_MARKERS if args.private_review else {}))
    if not args.timeline_only:
        require(args.baseline_dir and args.baseline_manifest and args.candidate_dir, 'Final oracle needs baseline and candidate directories')
        report['baseline'] = preserved_baseline(args.baseline_dir, args.baseline_manifest)
        report['assets'] = published_assets(args.candidate_dir, timing, args.require_audio, args.timing, args.private_review)
        if args.require_audio:
            require(args.timing and args.narration_alignment, 'Actual timestamp/audio provenance required')
            report['narration'] = narration_oracle(args.narration_alignment, timing, args.timing, args.private_review)
            require(report['assets']['music'] == report['narration']['musicCredit'], 'Exported credits differ from audio provenance')
            require(report['assets']['musicArrangement'] is None
                    or report['narration']['continuousMusic'] is not None,
                    'Published continuous arrangement lacks its independent audio proof')
            if report['narration']['continuousMusic']:
                continuous = report['narration']['continuousMusic']
                lineage = report['assets']['musicArrangement']
                require(lineage == dict(sourceSha256=continuous['sourceSha256'],
                                        editMapSha256=continuous['editMapSha256'],
                                        arrangementSha256=continuous['preparedMusicSha256'],
                                        modificationDisclosure=continuous['modificationDisclosure']),
                        'Published arrangement disclosure differs from the proved soundtrack')
        report['formats'] = {}
        for kind in ('mp4', 'webm'):
            candidate = args.candidate_dir / f'trailer.{kind}'
            report['formats'][kind] = video_oracle(candidate, timing, args.require_audio)
            if report['assets']['music']:
                tags = probe(candidate)['format'].get('tags', {})
                require(report['assets']['music']['credit'] in tags.values(), 'Export lacks music attribution metadata')
            if args.require_audio:
                require(args.reference_video_dir, 'Narrated output needs reviewed silent reference')
                require(frame_hashes(candidate) == frame_hashes(args.reference_video_dir / candidate.name), 'Picture changed during narration mux')
                report['formats'][kind]['audioPacketHash'] = audio_packet_oracle(
                    candidate, Path(report['narration']['finalAudioPath']),
                    continuous_music=report['narration']['continuousMusic'] is not None,
                    opus_frame_duration=report['narration']['audioEncoding'].get('opusFrameDurationMs'))
    if args.audit:
        write_json(args.audit, report)
    print('PRIVATE nonpublishable media oracles passed; publication rights unresolved' if args.private_review else
          'Machine media oracles passed; visual/audio/browser owner review remains separate')
