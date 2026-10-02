#!/usr/bin/env python3
"""Prove source-frame, pose, and bounded audio preservation for a private trailer candidate."""

import argparse
import hashlib
import importlib.util
import json
import math
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

from media_contract import SOURCES, bounded_process, probe, read_timing, require, run, write_json
from narration_proof import sha256
from opening_audio import verify_pcm16_tail_preserved
from soundtrack_proof import enforce_balance, measure_balance, samples
from trailer_scenes import (ENDING_END_FRAME, ENDING_START_FRAME, FPS, OPENING_FRAMES, SIZE,
                            load_brand_frames, render_timeline_frame, timed_shots)


WEB_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = WEB_ROOT.parent
UNCHANGED_FRAME_COUNT = 45 * FPS - OPENING_FRAMES - (ENDING_END_FRAME - ENDING_START_FRAME)
BOUNDARY_FRAMES = (0, 47, 48, 1111, 1112, 1349)
NODE_POSES = r"""
const fs = require('node:fs');
const ts = require('typescript');
const source = fs.readFileSync(process.argv[1], 'utf8');
const module = { exports: {} };
new Function('module', 'exports', ts.transpile(source, {
  target: ts.ScriptTarget.ES2020,
  module: ts.ModuleKind.CommonJS,
}))(module, module.exports);
const motion = module.exports;
const poses = Array.from({ length: 49 }, (_, frame) => ({
  frame,
  time: frame === 48 ? 1.6 : frame / 30,
  ...motion.surfaceTension(frame === 48 ? 1.6 : frame / 30),
}));
process.stdout.write(JSON.stringify({ rest: motion.REST, poses }));
"""


def node_poses(source_path):
    result = subprocess.run(['node', '-e', NODE_POSES, str(source_path)], cwd=WEB_ROOT,
                            check=True, capture_output=True, text=True, timeout=60)
    return json.loads(result.stdout)


def validate_pose_export(brand_directory, source_path):
    manifest_path = brand_directory / 'brand-frames.json'
    manifest = json.loads(manifest_path.read_text(encoding='utf-8-sig'))
    expected_hash = sha256(source_path)
    require(manifest['source']['sha256'] == expected_hash, 'Pose export source hash is stale')
    expected = node_poses(source_path)
    require(len(manifest['poses']) == len(expected['poses']) == 49, 'Expected 49 exported source poses')
    for actual, expected_pose in zip(manifest['poses'], expected['poses']):
        require(all(actual.get(key) == expected_pose[key] for key in ('frame', 'time', 'path', 'scaleX', 'scaleY', 'y')),
                f'Exported brand pose differs from src/brand/motion.ts at frame {expected_pose["frame"]}')
    final = manifest['poses'][48]
    require(final['path'] == expected['rest']['path'] and final['scaleX'] == final['scaleY'] == 1 and final['y'] == 0,
            'Frame 48 does not return the exact authoritative REST pose')
    brand_frames = load_brand_frames(brand_directory, source_path)
    return dict(sourceSha256=expected_hash, manifestSha256=sha256(manifest_path), poseCount=49,
                restFrame=48, exactPoses=True)


def residual(first, second):
    difference = ImageChops.difference(first, second)
    extrema = difference.getextrema()
    histogram = difference.convert('L').histogram()
    return dict(maxChannelResidual=max(value[1] for value in extrema),
                meanAbsoluteChannelResidual=sum(ImageStat.Stat(difference).mean) / 3,
                differingPixels=sum(histogram[1:]))


def raw_frame_audit(baseline_ref, source_paths, font, timing, brand_frames):
    source = run(['git', 'show', f'{baseline_ref}:web/media-src/trailer_scenes.py'],
                 timeout=30, capture=True).decode('utf-8')
    with tempfile.TemporaryDirectory(prefix='domios-framing-baseline-') as temporary:
        baseline_path = Path(temporary) / 'baseline_trailer_scenes.py'
        baseline_path.write_text(source, encoding='utf-8')
        spec = importlib.util.spec_from_file_location('baseline_trailer_scenes', baseline_path)
        baseline = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(baseline)
        old_shots = baseline.timed_shots(timing)
        new_shots = timed_shots(timing)
        require(old_shots == new_shots, 'Source shot timing/camera authority changed')
        frame_number = 0
        unchanged_hash = hashlib.sha256()
        changed_frames = 0
        changed_pixels = 0
        changed_residual_sum = 0.0
        changed_residual_max = 0
        boundaries = {}
        boundary_set = set(BOUNDARY_FRAMES)
        for old_edit, edit in zip(old_shots, new_shots):
            old_frames = baseline.source_frames(source_paths, old_edit)
            for index, frame in enumerate(old_frames):
                time = index / FPS
                current_frame = round((edit['at'] + time) * FPS)
                require(current_frame == frame_number, f'Frame timeline gap or overlap at {frame_number}')
                old_image = baseline.render(old_edit, time, frame, font)
                new_image = render_timeline_frame(edit, time, frame, font, brand_frames)
                require(old_image.size == new_image.size == SIZE, f'Unexpected rendered frame size at {frame_number}')
                old_bytes, new_bytes = old_image.tobytes(), new_image.tobytes()
                is_changed = 0 <= frame_number < OPENING_FRAMES or ENDING_START_FRAME <= frame_number < ENDING_END_FRAME
                if is_changed:
                    metrics = residual(old_image, new_image)
                    changed_frames += 1
                    changed_pixels += metrics['differingPixels']
                    changed_residual_sum += metrics['meanAbsoluteChannelResidual']
                    changed_residual_max = max(changed_residual_max, metrics['maxChannelResidual'])
                else:
                    require(old_bytes == new_bytes, f'Raw source renderer differs outside approved ranges at frame {frame_number}')
                    unchanged_hash.update(frame_number.to_bytes(4, 'big'))
                    unchanged_hash.update(old_bytes)
                if frame_number in boundary_set:
                    source_time = baseline.source_time(old_edit, time)
                    camera = baseline.camera_at(old_edit, time) if old_edit['scene'] == 'footage' else None
                    crop = baseline.crop_at(old_edit, time) if old_edit['scene'] == 'footage' else None
                    boundaries[str(frame_number)] = dict(time=frame_number / FPS, shot=old_edit['name'],
                        source=old_edit['source'], sourceTime=source_time, camera=camera, crop=crop,
                        baselineRawSha256=hashlib.sha256(old_bytes).hexdigest(),
                        candidateRawSha256=hashlib.sha256(new_bytes).hexdigest(),
                        residual=residual(old_image, new_image))
                frame_number += 1
        require(frame_number == 45 * FPS, f'Expected 1350 rendered frames, got {frame_number}')
        require(changed_frames == OPENING_FRAMES + ENDING_END_FRAME - ENDING_START_FRAME,
                f'Changed frame count differs from exact range contract: {changed_frames}')
        require(UNCHANGED_FRAME_COUNT == frame_number - changed_frames, 'Unexpected unchanged frame count')
        return dict(frameCount=frame_number, fps=FPS,
                    changedFrameRanges=[[0, OPENING_FRAMES], [ENDING_START_FRAME, ENDING_END_FRAME]],
                    changedFrameCount=changed_frames, changedFrameResidual=dict(
                        meanAbsoluteChannelResidual=changed_residual_sum / changed_frames,
                        maximumChannelResidual=changed_residual_max, differingPixels=changed_pixels),
                    unchangedFrameCount=UNCHANGED_FRAME_COUNT,
                    exactUnchangedSequenceSha256=unchanged_hash.hexdigest(), boundaries=boundaries)


def lineage_audit(baseline_proof_path, candidate_proof_path, timing_path):
    baseline = json.loads(baseline_proof_path.read_text(encoding='utf-8-sig'))
    candidate = json.loads(candidate_proof_path.read_text(encoding='utf-8-sig'))
    edit = candidate.get('openingEditLineage')
    require(isinstance(edit, dict), 'Candidate proof is missing bounded opening edit lineage')
    require(edit['baselineAudioProof'] == dict(path=str(baseline_proof_path.resolve()), sha256=sha256(baseline_proof_path)),
            'Candidate proof does not reference the supplied immutable audio proof')
    require(baseline['timingSha256'] == candidate['timingSha256'] == sha256(timing_path),
            'Audio proof timing authority changed')
    for key in ('sourceAudio', 'sourceAlignment', 'alignedVoice'):
        require(candidate[key] == baseline[key], f'Baseline voice proof chain changed: {key}')
    require(candidate['sourceMusic'] == baseline['sourceMusic'], 'Original arranged music provenance changed')
    require(candidate['musicInput'] == baseline.get('musicInput'), 'Original arranged music preparation proof changed')
    require(edit['baselineFinalAudio'] == baseline['finalAudio'], 'Original final mix record changed')
    require(edit['baselineMasteredVoice'] == baseline['mixStems']['voiceMastered'], 'Original mastered voice record changed')
    require(edit['baselineMasteredMusic'] == baseline['mixStems']['musicMastered'], 'Original mastered music record changed')
    require(candidate['music']['sha256'] == baseline['music']['sha256'], 'Approved arrangement SHA changed')
    require(candidate['music'] == baseline['music'], 'Visible music attribution or source credit changed')
    for key, value in baseline['music'].items():
        if key != 'credit':
            require(candidate['music'].get(key) == value, f'Original music provenance changed: {key}')
    arranged = Path(edit['arrangedMusic']['path'])
    opening = Path(edit['openingSource']['path'])
    require(sha256(arranged) == edit['arrangedMusic']['sha256'] == candidate['music']['sha256'],
            'Arranged music hash differs from credit.sha256')
    require(sha256(opening) == edit['openingSource']['sha256'] == candidate['music']['extractedAudioSha256'],
            'Opening source hash differs from credit.extractedAudioSha256')
    require(edit['openingSource']['validatedAgainst'] == 'credit.extractedAudioSha256'
            and edit['arrangedMusic']['validatedAgainst'] == 'credit.sha256', 'Opening source hash roles were conflated')
    require(edit['sourceExcerpt']['startSeconds'] == 2.6 and edit['sourceExcerpt']['endSeconds'] == 6.6
            and edit['sourceExcerpt']['startSample'] == round(2.6 * 48000)
            and edit['sourceExcerpt']['sampleCount'] == round(4 * 48000), 'Opening excerpt selection changed')
    require(edit['outputRange'] == dict(startSample=0, endSample=4 * 48000, startSeconds=0, endSeconds=4),
            'Opening output sample range changed')
    require(edit['crossfade']['startSample'] < edit['crossfade']['endSample'] == 4 * 48000
            and edit['crossfade']['startSample'] == round(3.25 * 48000)
            and edit['crossfade']['endSeconds'] == 4 and edit['crossfade']['durationSeconds'] == .75,
            'Opening crossfade does not reach the original music by four seconds')
    require(edit['antiClickRamp']['startSample'] == 0 and edit['antiClickRamp']['endSample'] == round(.012 * 48000),
            'Opening anti-click ramp changed')
    for key in ('sourceAudio', 'sourceAlignment', 'alignedVoice', 'finalAudio'):
        record = baseline[key]
        require(sha256(Path(record['path'])).lower() == record['sha256'].lower(),
                f'Baseline narration proof source changed: {key}')
    require(candidate['sourceAudio']['sha256'] == baseline['sourceAudio']['sha256'], 'Narration source hash changed')
    return dict(baselineAudioProofSha256=sha256(baseline_proof_path),
                originalVoiceSourceSha256=candidate['sourceAudio']['sha256'],
                originalAlignmentSha256=candidate['sourceAlignment']['sha256'],
                arrangedMusicSha256=edit['arrangedMusic']['sha256'],
                openingSourceSha256=edit['openingSource']['sha256'],
                sourceExcerpt=edit['sourceExcerpt'], outputRange=edit['outputRange'],
                crossfade=edit['crossfade'], antiClickRamp=edit['antiClickRamp'],
                gain=edit['gain'], changedSampleRange=edit['changedSampleRange'],
                unchangedMusicSampleRange=edit['unchangedMusicSampleRange'])


def audio_audit(baseline_proof_path, candidate_proof_path, timing_path):
    baseline = json.loads(baseline_proof_path.read_text(encoding='utf-8-sig'))
    candidate = json.loads(candidate_proof_path.read_text(encoding='utf-8-sig'))
    timing = read_timing(timing_path)
    count = round(timing['duration'] * 48000)
    edit = candidate['openingEditLineage']
    opening_end = round(edit['outputRange']['endSeconds'] * 48000)
    require(edit['changedSampleRange'] == [0, opening_end], 'Opening changed-sample extent is not bounded')
    require(edit['unchangedMusicSampleRange'] == [opening_end, count], 'Post-opening sample range is incomplete')
    require(edit['finalAudioTailPreserved'] is True and edit['finalAudioTailRange'] == [opening_end, count],
            'Final PCM tail preservation proof is missing or unbounded')
    verify_pcm16_tail_preserved(baseline['finalAudio']['path'], candidate['finalAudio']['path'], opening_end, count)
    voice_before = samples(baseline['mixStems']['voiceMastered'], count)
    voice_after = samples(candidate['mixStems']['voiceMastered'], count)
    music_before = samples(baseline['mixStems']['musicMastered'], count)
    music_after = samples(candidate['mixStems']['musicMastered'], count)
    require(baseline['mixStems']['voiceMastered']['sha256'] == candidate['mixStems']['voiceMastered']['sha256'],
            'Mastered voice file bytes changed')
    require(voice_before == voice_after, 'Mastered voice PCM sample changed')
    require(music_before[opening_end:] == music_after[opening_end:], 'Mastered music PCM after 4s changed')
    onset100 = round(.1 * 48000)
    onset650 = round(.65 * 48000)
    rms100 = math.sqrt(sum(value * value for value in music_after[:onset100]) / onset100)
    rms650 = math.sqrt(sum(value * value for value in music_after[:onset650]) / onset650)
    peak100 = max(abs(value) for value in music_after[:onset100])
    peak650 = max(abs(value) for value in music_after[:onset650])
    require(rms100 > 0 and 20 * math.log10(rms100) > -50, 'First 100ms music RMS must exceed -50dBFS')
    require(rms650 > 0 and 20 * math.log10(rms650) > -40, 'First 650ms music RMS must exceed -40dBFS')
    balance = measure_balance(timing, candidate['mixStems'], candidate['finalAudio'], candidate['mastering'])
    require(balance == candidate['phraseBalance'], 'Candidate balance proof differs from actual stem samples')
    enforce_balance(balance)
    return dict(sampleRate=48000, sampleCount=count, openingEndSample=opening_end,
                voicePcmSha256Before=baseline['mixStems']['voiceMastered']['sha256'],
                voicePcmSha256After=candidate['mixStems']['voiceMastered']['sha256'],
                voiceSamplesIdentical=True, musicSamplesAfterOpeningIdentical=True,
                finalAudioSamplesAfterOpeningIdentical=True,
                first100ms=dict(rms=rms100, dbfs=20 * math.log10(rms100), peak=peak100),
                first650ms=dict(rms=rms650, dbfs=20 * math.log10(rms650), peak=peak650),
                phraseBalance=balance, mastering=candidate['mastering'])


def video_probe_audit(baseline_ref, candidate_proof_path):
    proof = json.loads(candidate_proof_path.read_text(encoding='utf-8-sig'))
    reference = proof.get('pictureReference')
    require(isinstance(reference, dict) and reference.get('videos'), 'Candidate proof lacks silent picture references')
    candidate = Path(reference['videos']['mp4']['path'])
    require(sha256(candidate) == reference['videos']['mp4']['sha256'], 'Silent candidate MP4 changed after narration mux')
    with tempfile.TemporaryDirectory(prefix='domios-decoded-reference-') as temporary:
        baseline = Path(temporary) / 'baseline.mp4'
        with baseline.open('wb') as target:
            result = subprocess.run(['git', 'show', f'{baseline_ref}:web/public/media/trailer.mp4'],
                                    cwd=REPO_ROOT, stdout=target, stderr=subprocess.PIPE, timeout=120)
        require(result.returncode == 0 and baseline.stat().st_size > 0,
                f'Cannot read baseline encoded MP4 from {baseline_ref}: {result.stderr.decode(errors="replace")}')
        baseline_info, candidate_info = probe(baseline), probe(candidate)
        def video_stream(info):
            streams = [stream for stream in info['streams'] if stream['codec_type'] == 'video']
            require(len(streams) == 1 and streams[0]['width'] == SIZE[0] and streams[0]['height'] == SIZE[1],
                    'Decoded residual comparison requires a single 1280x720 video stream')
            require(int(streams[0]['nb_read_frames']) == 45 * FPS, 'Encoded picture has an unexpected frame count')
            return streams[0]

        video_stream(baseline_info)
        video_stream(candidate_info)
        command = lambda path: ['ffmpeg', '-v', 'error', '-i', path, '-map', '0:v:0', '-fps_mode', 'passthrough',
                                '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-']
        frame_bytes = SIZE[0] * SIZE[1] * 3
        count = 0
        groups = dict(unchanged=_residual_bucket(), opening=_residual_bucket(), ending=_residual_bucket())
        boundaries = {}
        with bounded_process(command(baseline), 900, stdout=subprocess.PIPE) as old_process, \
             bounded_process(command(candidate), 900, stdout=subprocess.PIPE) as new_process:
            for index in range(45 * FPS):
                old_data, new_data = old_process.stdout.read(frame_bytes), new_process.stdout.read(frame_bytes)
                require(len(old_data) == len(new_data) == frame_bytes, f'Encoded decode ended early at frame {index}')
                old_image = Image.frombytes('RGB', SIZE, old_data)
                new_image = Image.frombytes('RGB', SIZE, new_data)
                metrics = residual(old_image, new_image)
                key = 'opening' if index < OPENING_FRAMES else ('ending' if index >= ENDING_START_FRAME else 'unchanged')
                _add_residual(groups[key], metrics)
                if index in BOUNDARY_FRAMES:
                    boundaries[str(index)] = metrics
                count += 1
            for process in (old_process, new_process):
                require(not process.stdout.read(frame_bytes), 'Encoded decode contains frames beyond the 45s timeline')
                process.stdout.close()
                require(process.wait(timeout=10) == 0, 'Encoded residual decode failed')
        return dict(frameCount=count, baselineEncodedSha256=sha256(baseline),
                    candidateSilentEncodedSha256=sha256(candidate),
                    interpretation='Decoded residuals are measured only; encoder output is not asserted byte- or pixel-identical.',
                    ranges={key: _finish_bucket(value) for key, value in groups.items()}, boundaries=boundaries)


def _residual_bucket():
    return dict(frames=0, differingPixels=0, meanSum=0.0, maximumChannelResidual=0)


def _add_residual(bucket, metrics):
    bucket['frames'] += 1
    bucket['differingPixels'] += metrics['differingPixels']
    bucket['meanSum'] += metrics['meanAbsoluteChannelResidual']
    bucket['maximumChannelResidual'] = max(bucket['maximumChannelResidual'], metrics['maxChannelResidual'])


def _finish_bucket(bucket):
    return dict(frames=bucket['frames'], differingPixels=bucket['differingPixels'],
                meanAbsoluteChannelResidual=bucket['meanSum'] / bucket['frames'],
                maximumChannelResidual=bucket['maximumChannelResidual'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline-ref', required=True)
    parser.add_argument('--source-dir', type=Path, required=True)
    parser.add_argument('--font', type=Path, required=True)
    parser.add_argument('--timing', type=Path, required=True)
    parser.add_argument('--brand-frames', type=Path, required=True)
    parser.add_argument('--baseline-audio-proof', type=Path, required=True)
    parser.add_argument('--candidate-audio-proof', type=Path, required=True)
    parser.add_argument('--audit', type=Path, required=True)
    args = parser.parse_args()
    require(args.font.is_file(), 'Required installed font is missing')
    timing = read_timing(args.timing)
    require(timing['duration'] == 45 and timing['fps'] == FPS, 'Framing proof requires the approved 45s/30fps cut')
    paths = {key: args.source_dir / name for key, name in SOURCES.items()}
    for path in paths.values():
        require(path.is_file(), f'Missing unchanged source recording: {path}')
    source_hashes = {key: sha256(path) for key, path in paths.items()}
    source_path = WEB_ROOT / 'src/brand/motion.ts'
    poses = validate_pose_export(args.brand_frames, source_path)
    brand_frames = load_brand_frames(args.brand_frames, source_path)
    raw = raw_frame_audit(args.baseline_ref, paths, args.font, timing, brand_frames)
    lineage = lineage_audit(args.baseline_audio_proof, args.candidate_audio_proof, args.timing)
    audio = audio_audit(args.baseline_audio_proof, args.candidate_audio_proof, args.timing)
    decoded = video_probe_audit(args.baseline_ref, args.candidate_audio_proof)
    report = dict(baselineRef=args.baseline_ref, timingSha256=sha256(args.timing), sourceHashes=source_hashes,
                  poses=poses, rawRenderer=raw, decodedPictureResiduals=decoded,
                  audioLineage=lineage, audioPreservation=audio,
                  listening='Objective proof only; root must audition the full candidate and judge musical presence/clicks.')
    write_json(args.audit, report)
    print(json.dumps(dict(frames=raw['frameCount'], unchangedFrames=raw['unchangedFrameCount'],
                          changedRanges=raw['changedFrameRanges'], voicePcmPreserved=True,
                          post4sMusicPreserved=True, first100msDbfs=audio['first100ms']['dbfs'],
                          first650msDbfs=audio['first650ms']['dbfs']), indent=2))


if __name__ == '__main__':
    main()
