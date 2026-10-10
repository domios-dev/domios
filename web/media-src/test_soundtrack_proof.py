import math
import struct
import tempfile
import unittest
import wave
from pathlib import Path

from soundtrack_proof import RATE, audio_record, enforce_balance, measure_balance


class SoundtrackProofTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='trailer-sound-proof-')
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.phrase_samples = RATE // 10
        self.count = self.phrase_samples * 13
        self.timing = dict(duration=self.count / RATE, cues=[], narration=dict(words=[]))
        for index in range(13):
            self.timing['cues'].append(dict(id=index + 1, text=f'phrase {index + 1}'))
            self.timing['narration']['words'].append(dict(cueId=index + 1, finalStart=index / 10,
                                                        finalEnd=(index + 1) / 10))
        self.mastering = dict(linearGain=.5, gainDb=20 * math.log10(.5))

    def record(self, name, values):
        path = self.root / f'{name}.wav'
        with wave.open(str(path), 'wb') as output:
            output.setnchannels(1)
            output.setsampwidth(2)
            output.setframerate(RATE)
            output.writeframes(struct.pack(f'<{len(values)}h', *values))
        return audio_record(path)

    def fixture(self, music_value=1410, voice_value=8192, loud_phrase=None):
        voice = [voice_value] * self.count
        music = [music_value] * self.count
        if loud_phrase is not None:
            start = (loud_phrase - 1) * self.phrase_samples
            music[start:start + self.phrase_samples] = [1840] * self.phrase_samples
        stems = dict(voicePostDuck=self.record('voice-post', voice),
                     musicPostDuck=self.record('music-post', music),
                     voiceMastered=self.record('voice-master', [value // 2 for value in voice]),
                     musicMastered=self.record('music-master', [value // 2 for value in music]),
                     unmasteredMix=self.record('mix', [a + b for a, b in zip(voice, music)]))
        final = self.record('final', [(a + b) // 2 for a, b in zip(voice, music)])
        return stems, final

    def measure(self, stems, final):
        return measure_balance(self.timing, stems, final, self.mastering)

    def test_actual_levels_above_threshold(self):
        stems, final = self.fixture()
        report = self.measure(stems, final)
        enforce_balance(report)
        self.assertEqual(len(report['phrases']), 13)
        self.assertAlmostEqual(report['minimumMeasuredDb'], 20 * math.log10(8192 / 1410))
        self.assertTrue(all(phrase['status'] == 'measured' for phrase in report['phrases']))

    def test_one_loud_phrase_fails_even_with_quiet_programme(self):
        stems, final = self.fixture(loud_phrase=7)
        report = self.measure(stems, final)
        self.assertEqual([phrase['cueId'] for phrase in report['phrases'] if not phrase['passed']], [7])
        with self.assertRaisesRegex(ValueError, 'cue 7'):
            enforce_balance(report)

    def test_silent_music_is_explicit_and_allowed(self):
        stems, final = self.fixture(music_value=0)
        report = self.measure(stems, final)
        enforce_balance(report)
        self.assertIsNone(report['minimumMeasuredDb'])
        self.assertTrue(all(phrase['status'] == 'music-silent' and phrase['musicRms'] == 0
                            and phrase['separationDb'] is None for phrase in report['phrases']))

    def test_silent_voice_never_passes(self):
        for music_value in (0, 1410):
            with self.subTest(music_value=music_value):
                stems, final = self.fixture(music_value=music_value, voice_value=0)
                report = self.measure(stems, final)
                with self.assertRaisesRegex(ValueError, 'voice-silent'):
                    enforce_balance(report)

    def test_independent_stem_mastering_fails(self):
        stems, final = self.fixture()
        stems['musicMastered'] = self.record('music-independent', [1410] * self.count)
        final = self.record('independent-final', [4096 + 1410] * self.count)
        with self.assertRaisesRegex(ValueError, 'same mix graph/shared gain'):
            self.measure(stems, final)

    def test_sum_only_mastering_fails(self):
        stems, final = self.fixture()
        final = self.record('changed-final', [4700] * self.count)
        with self.assertRaisesRegex(ValueError, 'Final WAV differs'):
            self.measure(stems, final)

    def test_changed_stem_hash_fails(self):
        stems, final = self.fixture()
        self.record('music-post', [0] * self.count)
        with self.assertRaisesRegex(ValueError, 'proof file changed'):
            self.measure(stems, final)


if __name__ == '__main__':
    unittest.main()
