import json
import tempfile
import unittest
import wave
from pathlib import Path

from music_review import (PRIVATE_MARKERS, PRIVATE_NOTICE, credit_payload, prepared_music,
                          protect_private_path, validate_markers, validate_music, validate_license, validate_prepared_info)
from narration_proof import sha256


class MusicReviewTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix='trailer-private-validator-')
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        source, output = self.root / 'source-fixture', self.root / 'arranged-fixture'
        source.write_bytes(b'synthetic source fixture')
        output.write_bytes(b'synthetic arranged fixture')
        self.arrangement = self.root / 'arrangement.json'
        self.arrangement.write_text(json.dumps(dict(source=str(source), sourceSha256=sha256(source),
                                                    output=str(output), outputSha256=sha256(output))), encoding='utf-8')
        self.private = dict(title='Private fixture', author='Generated fixture', credit='Private fixture; terms unresolved.',
                            sourcePage=None, license=None, licenseUrl=None, sha256=sha256(output),
                            originalMediaSha256='1' * 64, extractedAudioSha256=sha256(source),
                            arrangement=dict(path=str(self.arrangement), sha256=sha256(self.arrangement)), **PRIVATE_MARKERS)
        self.licensed = dict(title='Licensed fixture', author='Fixture author', credit='Fixture credit', sha256='3' * 64,
                             sourcePage='https://example.test/source', license='CC BY 4.0',
                             licenseUrl='https://creativecommons.org/licenses/by/4.0/')

    def test_licensed_normal_mode_unchanged(self):
        self.assertEqual(validate_music(self.licensed), self.licensed)
        self.assertEqual(credit_payload(self.licensed)['credit'], self.licensed['credit'])
        validate_markers({})

    def test_private_null_rights_preserved_with_explicit_notice(self):
        validated = validate_music(self.private, True)
        payload = credit_payload(validated, True)
        self.assertIsNone(payload['sourcePage'])
        self.assertIsNone(payload['license'])
        self.assertIsNone(payload['licenseUrl'])
        self.assertTrue(payload['reviewOnly'])
        self.assertFalse(payload['publishable'])
        self.assertIn(PRIVATE_NOTICE, payload['credit'])

    def test_default_rejects_private_metadata_and_package(self):
        with self.assertRaisesRegex(ValueError, 'Publication rejects'):
            validate_music(self.private)
        with self.assertRaisesRegex(ValueError, 'Publication rejects'):
            validate_markers(PRIVATE_MARKERS)
        with self.assertRaises(ValueError):
            validate_markers(dict(rightsStatus='unresolved'))
        with self.assertRaises(ValueError):
            validate_license(dict(self.licensed, license=None, licenseUrl=None))

    def test_missing_or_ambiguous_private_markers_fail(self):
        for key, wrong in [('reviewOnly', 'true'), ('publishable', None), ('rightsStatus', 'licensed')]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate_music(dict(self.private, **{key: wrong}), True)
        with self.assertRaises(ValueError):
            validate_music(self.licensed, True)

    def test_unresolved_mode_rejects_fabricated_license(self):
        with self.assertRaisesRegex(ValueError, 'must not claim'):
            validate_music(dict(self.private, license='CC BY 4.0'), True)
        with self.assertRaises(ValueError):
            validate_music(dict(self.private, licenseUrl='https://example.test/fake-license'), True)

    def test_changed_arrangement_record_fails(self):
        self.arrangement.write_text('{}', encoding='utf-8')
        with self.assertRaisesRegex(ValueError, 'Arrangement record changed'):
            validate_music(self.private, True)

    def test_arrangement_cannot_point_at_different_prepared_track(self):
        with self.assertRaisesRegex(ValueError, 'Arrangement output provenance differs'):
            validate_music(dict(self.private, sha256='0' * 64), True)

    def test_public_archived_and_input_overlap_paths_fail(self):
        public = self.root / 'public'
        inputs = self.root / 'preserved-input'
        for path in (public, public / 'media', public / 'media/private', self.root / 'archive/new',
                     self.root / 'trailer-before-fixture/new', inputs, inputs / 'new', self.root):
            with self.subTest(path=path), self.assertRaises(ValueError):
                protect_private_path(path, [inputs], public)
        protect_private_path(self.root / 'private-preview/narrated', [inputs], public)

    def test_real_pcm_prepared_extent_and_short_track(self):
        for duration in (12, 45):
            path = self.root / f'prepared-{duration}.wav'
            with wave.open(str(path), 'wb') as audio:
                audio.setnchannels(2)
                audio.setsampwidth(2)
                audio.setframerate(48000)
                audio.writeframes(bytes(duration * 48000 * 4))
            if duration == 12:
                with self.assertRaisesRegex(ValueError, 'shorter.*looping is forbidden'):
                    prepared_music(path, 45)
            else:
                info = prepared_music(path, 45)
                self.assertEqual(info['sourceSamples'], 2160000)
                self.assertEqual(info['consumedSamples'], 2160000)
                self.assertFalse(info['looped'])

    def test_prepared_rejects_compressed_wrong_rate_or_missing_sample(self):
        stream = dict(codec_type='audio', codec_name='pcm_f32le', sample_rate='48000',
                      duration_ts=2160000, time_base='1/48000')
        for change in (dict(codec_name='aac'), dict(sample_rate='44100'), dict(duration_ts=2159999)):
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_prepared_info(dict(streams=[dict(stream, **change)]), 45)


if __name__ == '__main__':
    unittest.main()
