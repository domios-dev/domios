import tempfile
import unittest
from array import array
from pathlib import Path

from continuous_music import (MINIMUM_JOIN_SAMPLES, PROGRAM_SAMPLES, RATE, SCHEMA,
                              require_source_hash, validate_edit_map)
from music_review import validate_markers
from soundtrack_proof import (enforce_music_continuity, measure_music_continuity,
                              require_complete_phrase_balance, require_fixed_mastering,
                              require_reconstruction_residual, verify_exact_samples, verify_record)


class ContinuousMusicContractTests(unittest.TestCase):
    def setUp(self):
        self.output_count = 180_000
        source_ranges = ((10_000, 70_000), (70_000, 130_000),
                         (140_000, 200_000), (210_000, 270_000))
        output_starts = (0, 40_000, 80_000, 120_000)
        self.edit_map = dict(
            schema=SCHEMA,
            sourceSha256="a" * 64,
            sampleRate=RATE,
            outputSampleCount=self.output_count,
            segments=[
                dict(id=f"cut-{index + 1}", sourceStartSample=source_start,
                     sourceEndSample=source_end, outputStartSample=output_start)
                for index, ((source_start, source_end), output_start)
                in enumerate(zip(source_ranges, output_starts))
            ],
            joins=[
                dict(leftSegment="cut-1", rightSegment="cut-2", startSample=40_000,
                     endSample=60_000, curve="equalPower"),
                dict(leftSegment="cut-2", rightSegment="cut-3", startSample=80_000,
                     endSample=100_000, curve="equalPower"),
                dict(leftSegment="cut-3", rightSegment="cut-4", startSample=120_000,
                     endSample=140_000, curve="equalPower"),
            ],
            gainEnvelope=[dict(sample=0, gainDb=-6),
                          dict(sample=self.output_count - 1, gainDb=-6)],
            ducking=dict(attackSamples=15_000, releaseSamples=15_000, reductionDb=5),
            finalFade=dict(startSample=self.output_count - 9_600,
                           endSample=self.output_count, curve="cosine"),
        )
        self.silence = [dict(start=0, end=10_000 / RATE),
                        dict(start=130_000 / RATE, end=140_000 / RATE)]

    def test_source_hash_is_bound_to_the_scouted_audio(self):
        require_source_hash(self.edit_map, "a" * 64)
        with self.assertRaises(ValueError):
            require_source_hash(self.edit_map, "b" * 64)

    def test_measured_silent_source_range_is_rejected(self):
        bad = dict(self.edit_map)
        bad["segments"] = [dict(segment) for segment in self.edit_map["segments"]]
        bad["segments"][0]["sourceStartSample"] = 0
        with self.assertRaises(ValueError):
            validate_edit_map(bad, 300_000, self.output_count, self.silence)

    def test_each_cut_requires_its_measured_overlapping_join(self):
        validate_edit_map(self.edit_map, 300_000, self.output_count, self.silence)
        bad = dict(self.edit_map)
        bad["joins"] = self.edit_map["joins"][:-1]
        with self.assertRaises(ValueError):
            validate_edit_map(bad, 300_000, self.output_count, self.silence)
        bad = dict(self.edit_map)
        bad["joins"] = [dict(join) for join in self.edit_map["joins"]]
        bad["joins"][0]["endSample"] = bad["joins"][0]["startSample"] + MINIMUM_JOIN_SAMPLES - 1
        with self.assertRaises(ValueError):
            validate_edit_map(bad, 300_000, self.output_count, self.silence)

    def test_wrong_stem_hash_is_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "stem.wav"
            path.write_bytes(b"retained-stem")
            with self.assertRaises(ValueError):
                verify_record(dict(path=str(path), sha256="0" * 64), "music stem")

    def test_a_changed_voice_sample_is_rejected(self):
        expected = array("f", [0.1, -0.2, 0.3])
        actual = array("f", expected)
        actual[1] = -0.199
        with self.assertRaises(ValueError):
            verify_exact_samples(expected, actual, "voiceMastered")

    def test_a_missing_phrase_balance_is_rejected(self):
        complete = dict(phrases=[dict(cueId=index, passed=True) for index in range(1, 14)])
        require_complete_phrase_balance(complete, list(range(1, 14)))
        with self.assertRaises(ValueError):
            require_complete_phrase_balance(dict(phrases=complete["phrases"][:-1]), list(range(1, 14)))

    def test_three_seconds_of_silence_fails_the_dropout_floor(self):
        values = array("f", [0.1]) * PROGRAM_SAMPLES
        values[20 * RATE:23 * RATE] = array("f", [0]) * (3 * RATE)
        report = measure_music_continuity(values, PROGRAM_SAMPLES)
        with self.assertRaises(ValueError):
            enforce_music_continuity(report, "test music")

    def test_modified_shared_gain_is_rejected(self):
        prior = dict(gainDb=-1.0, linearGain=10 ** (-1 / 20), measurement=dict(input_i="-17.0"))
        changed = dict(prior, gainDb=-2.0, linearGain=10 ** (-2 / 20))
        with self.assertRaises(ValueError):
            require_fixed_mastering(changed, prior)

    def test_wrong_final_reconstruction_is_rejected(self):
        with self.assertRaises(ValueError):
            require_reconstruction_residual(0.1, 1 / 32768, "finalSum")

    def test_unresolved_publication_markers_are_rejected(self):
        with self.assertRaises(ValueError):
            validate_markers(dict(reviewOnly=True, publishable=False, rightsStatus="unresolved"), False)


if __name__ == "__main__":
    unittest.main()
