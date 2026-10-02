import json
import math
import subprocess
import tempfile
import unittest
from array import array
from pathlib import Path
from unittest.mock import patch

from PIL import Image, ImageChops, ImageStat

import trailer_scenes
from media_contract import prepare_output
from narration_proof import sha256
from opening_audio import (RATE, assemble_music, choose_gain, dbfs, rms, sum_samples,
                           shape_excerpt, verify_pcm16_tail_preserved, verify_preservation,
                           write_float_wav, write_pcm16_wav, write_pcm16_wav_preserving_tail)
from soundtrack_proof import enforce_balance, measure_balance


def tone(count, amplitude, frequency, start=0, end=None):
    end = count if end is None else end
    return array('f', (amplitude * math.sin(2 * math.pi * frequency * index / RATE)
                       if start <= index < end else 0 for index in range(count)))


def record(path):
    return dict(path=str(path), sha256=sha256(path))


class OpeningMixTests(unittest.TestCase):
    def test_existing_source_rejects_near_silent_negative_control_and_proves_stems(self):
        count = 5 * RATE
        end_sample = 4 * RATE
        crossfade_start = end_sample - round(.75 * RATE)
        ramp_samples = round(.012 * RATE)
        voice = tone(count, .4, 223, round(.65 * RATE), round(3.3 * RATE))
        baseline_music = tone(count, .02, 137, end_sample)
        excerpt = tone(end_sample, .18, 431)
        excerpt_term = shape_excerpt(excerpt, crossfade_start, ramp_samples)
        timing = dict(duration=5, cues=[dict(id=1, text='A measured opening')],
                      narration=dict(words=[dict(cueId=1, text='A', finalStart=.65, finalEnd=3.3)]))
        gain, report = choose_gain(baseline_music, excerpt_term, voice, timing, 4, count)
        candidate_music, _ = assemble_music(baseline_music, excerpt, gain, end_sample,
                                            crossfade_start, ramp_samples)
        self.assertGreater(dbfs(rms(candidate_music, 0, round(.1 * RATE))), -50)
        self.assertGreater(dbfs(rms(candidate_music, 0, round(.65 * RATE))), -40)
        self.assertEqual(candidate_music[end_sample:], baseline_music[end_sample:])
        self.assertEqual(candidate_music[0], baseline_music[0])
        self.assertEqual(report['selectedLinearGain'], gain)

        digital_silence = shape_excerpt(array('f', [0]) * end_sample, crossfade_start, ramp_samples)
        with self.assertRaisesRegex(ValueError, 'no measurable material at the beginning'):
            choose_gain(baseline_music, digital_silence, voice, timing, 4, count)

        with tempfile.TemporaryDirectory(prefix='domios-opening-proof-') as temporary:
            root = Path(temporary)
            voice_post = root / 'voice-post.wav'
            music_post = root / 'music-post.wav'
            voice_mastered = root / 'voice-mastered.wav'
            music_mastered = root / 'music-mastered.wav'
            unmastered = root / 'unmastered.wav'
            final = root / 'final.wav'
            write_float_wav(voice_post, voice)
            write_float_wav(music_post, candidate_music)
            write_float_wav(voice_mastered, voice)
            write_float_wav(music_mastered, candidate_music)
            write_float_wav(unmastered, sum_samples(voice, candidate_music))
            write_pcm16_wav(final, sum_samples(voice, candidate_music))
            stems = dict(voicePostDuck=record(voice_post), musicPostDuck=record(music_post),
                         voiceMastered=record(voice_mastered), musicMastered=record(music_mastered),
                         unmasteredMix=record(unmastered))
            balance = measure_balance(timing, stems, record(final), dict(linearGain=1, gainDb=0))
            enforce_balance(balance)
            self.assertTrue(balance['phrases'][0]['passed'])

    def test_preservation_rejects_voice_or_later_music_sample_changes(self):
        voice = array('f', [0.1, 0.2, 0.3, 0.4])
        music = array('f', [0.4, 0.3, 0.2, 0.1])
        verify_preservation(voice, array('f', voice), music, array('f', music), 2)
        wrong_voice = array('f', voice)
        wrong_voice[1] += .0001
        with self.assertRaisesRegex(ValueError, 'voice PCM changed'):
            verify_preservation(voice, wrong_voice, music, array('f', music), 2)
        wrong_music = array('f', music)
        wrong_music[2] += .0001
        with self.assertRaisesRegex(ValueError, 'after the bounded opening changed'):
            verify_preservation(voice, array('f', voice), music, wrong_music, 2)

    def test_final_mix_preserves_old_pcm_tail_and_rejects_one_lsb_change(self):
        with tempfile.TemporaryDirectory(prefix='domios-final-tail-') as temporary:
            root = Path(temporary)
            prior = root / 'prior.wav'
            candidate = root / 'candidate.wav'
            changed = root / 'changed.wav'
            prior_samples = array('f', [.1, .2, .3, .4, .5])
            candidate_samples = array('f', [.5, .6, .31, .41, .51])
            write_pcm16_wav(prior, prior_samples)
            write_pcm16_wav_preserving_tail(candidate, candidate_samples, prior, 2)
            verify_pcm16_tail_preserved(prior, candidate, 2, len(prior_samples))

            for index in (2, 3, 4):
                with self.subTest(changed_sample=index):
                    one_lsb_change = array('f', prior_samples)
                    one_lsb_change[index] += 1 / 32768
                    write_pcm16_wav(changed, one_lsb_change)
                    with self.assertRaisesRegex(ValueError, 'Final PCM16 tail after the opening boundary changed'):
                        verify_pcm16_tail_preserved(prior, changed, 2, len(prior_samples))


class FramingBoundaryTests(unittest.TestCase):
    def test_only_declared_frame_ranges_use_opening_and_finale(self):
        base = Image.new('RGB', (1280, 720), '#101820')
        ending = Image.new('RGB', (1280, 720), '#303840')
        brand = Image.new('RGBA', (256, 256), (88, 166, 255, 255))
        brand_frames = dict(images=[brand] * 49)
        edit = dict(at=0, duration=45, scene='footage', source='mesh-talk', title='')
        source = Image.new('RGB', (2048, 1396), '#18212a')
        with patch.object(trailer_scenes, 'render', side_effect=lambda *args: base.copy()), \
             patch.object(trailer_scenes, 'agent_ending', return_value=ending) as ending_render:
            frame0 = trailer_scenes.render_timeline_frame(edit, 0 / 30, source, None, brand_frames)
            fade_frames = [trailer_scenes.render_timeline_frame(edit, frame / 30, source, None, brand_frames)
                           for frame in range(trailer_scenes.OPENING_FADE_START_FRAME, 49)]
            frame47 = fade_frames[-2]
            frame48 = trailer_scenes.render_timeline_frame(edit, 48 / 30, source, None, brand_frames)
            frame1111 = trailer_scenes.render_timeline_frame(edit, 1111 / 30, source, None, brand_frames)
            frame1112 = trailer_scenes.render_timeline_frame(edit, 1112 / 30, source, None, brand_frames)
            frame1349 = trailer_scenes.render_timeline_frame(edit, 1349 / 30, source, None, brand_frames)
            frame1350 = trailer_scenes.render_timeline_frame(edit, 45, source, None, brand_frames)
        self.assertNotEqual(frame0.tobytes(), base.tobytes())
        fade_residuals = [sum(ImageStat.Stat(ImageChops.difference(frame, base)).mean) / 3
                          for frame in fade_frames]
        self.assertTrue(all(later < earlier for earlier, later in zip(fade_residuals, fade_residuals[1:])))
        self.assertGreater(fade_residuals[-2], 0)
        self.assertLessEqual(fade_residuals[-2] / fade_residuals[-3], .4)
        self.assertLess(fade_residuals[-2] / fade_residuals[0], .05)
        self.assertEqual(frame48.tobytes(), base.tobytes())
        self.assertEqual(frame1111.tobytes(), base.tobytes())
        self.assertEqual(frame1112.tobytes(), ending.tobytes())
        self.assertEqual(frame1349.tobytes(), ending.tobytes())
        self.assertEqual(frame1350.tobytes(), base.tobytes())
        self.assertEqual(ending_render.call_count, 2)

    def test_stale_pose_source_hash_and_nonempty_output_are_refused(self):
        source_path = Path(__file__).resolve().parents[1] / 'src/brand/motion.ts'
        with tempfile.TemporaryDirectory(prefix='domios-stale-poses-') as temporary:
            directory = Path(temporary)
            manifest = dict(schemaVersion=1, fps=30, duration=1.6,
                            source=dict(path=str(source_path), sha256='0' * 64), poses=[])
            (directory / 'brand-frames.json').write_text(json.dumps(manifest), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'source hash is stale'):
                trailer_scenes.load_brand_frames(directory, source_path)

        with tempfile.TemporaryDirectory(prefix='domios-nonempty-output-') as temporary:
            directory = Path(temporary)
            (directory / 'keep.txt').write_text('preserve', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'must be empty'):
                prepare_output(directory)
            self.assertEqual((directory / 'keep.txt').read_text(encoding='utf-8'), 'preserve')

    def test_edge_pose_export_refuses_nonempty_destination_before_launch(self):
        exporter = Path(__file__).with_name('export-threshold.mjs')
        with tempfile.TemporaryDirectory(prefix='domios-export-refusal-') as temporary:
            directory = Path(temporary)
            marker = directory / 'keep.txt'
            marker.write_text('preserve', encoding='utf-8')
            result = subprocess.run(['node', str(exporter), '--output-dir', str(directory), '--fps', '30',
                                     '--duration', '1.6'], capture_output=True, text=True, timeout=60)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Output directory must be empty', result.stderr)
            self.assertEqual(marker.read_text(encoding='utf-8'), 'preserve')


if __name__ == '__main__':
    unittest.main()
