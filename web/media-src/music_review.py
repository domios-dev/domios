import json
import re
from fractions import Fraction
from pathlib import Path

from media_contract import probe, require
from narration_proof import sha256


PRIVATE_MARKERS = dict(reviewOnly=True, publishable=False, rightsStatus='unresolved')
PRIVATE_NOTICE = 'Private review: reviewOnly=true; publishable=false; rightsStatus=unresolved.'


def validate_markers(record, private_review=False):
    if private_review:
        require(record.get('reviewOnly') is True and record.get('publishable') is False
                and record.get('rightsStatus') == 'unresolved', 'Private review requires explicit nonpublishable/unresolved markers')
    else:
        require(record.get('reviewOnly', False) is False and record.get('publishable', True) is True
                and record.get('rightsStatus', 'licensed') in ('licensed', 'cleared'),
                'Publication rejects private/review-only or unresolved rights')


def validate_music(credit, private_review=False):
    validate_markers(credit, private_review)
    for key in ('title', 'author', 'credit', 'sha256'):
        require(isinstance(credit.get(key), str) and bool(credit[key].strip()), f'Missing music provenance: {key}')
    require(re.fullmatch(r'[0-9a-fA-F]{64}', credit['sha256']), 'Invalid music SHA256')
    result = dict(credit)
    if private_review:
        require(credit.get('license') is None and credit.get('licenseUrl') is None,
                'Unresolved private music must not claim a license or license URL')
        for key in ('sourcePage', 'license', 'licenseUrl'):
            result.setdefault(key, None)
        require(result['sourcePage'] is None or isinstance(result['sourcePage'], str), 'Invalid music source provenance')
        for key in ('originalMediaSha256', 'extractedAudioSha256'):
            require(isinstance(credit.get(key), str) and re.fullmatch(r'[0-9a-fA-F]{64}', credit[key]),
                    f'Missing original music provenance: {key}')
        arrangement = credit.get('arrangement', {})
        require(bool(arrangement.get('path')) and bool(arrangement.get('sha256')), 'Missing arrangement path/hash')
        path = Path(arrangement['path'])
        require(sha256(path).lower() == arrangement['sha256'].lower(), 'Arrangement record changed')
        edits = json.loads(path.read_text(encoding='utf-8-sig'))
        require(edits['sourceSha256'].lower() == credit['extractedAudioSha256'].lower()
                == sha256(Path(edits['source'])).lower(), 'Arrangement source provenance differs')
        require(edits['outputSha256'].lower() == credit['sha256'].lower()
                == sha256(Path(edits['output'])).lower(), 'Arrangement output provenance differs')
    else:
        validate_license(credit)
    return result


def validate_license(credit):
    validate_markers(credit)
    for key in ('title', 'author', 'sourcePage', 'license', 'licenseUrl', 'credit'):
        require(bool(credit.get(key)), f'Missing music provenance: {key}')


def credit_payload(credit, private_review=False):
    if private_review:
        result = dict(credit)
        result['credit'] = credit['credit'].rstrip() + '\n' + PRIVATE_NOTICE
        return result
    return {key: credit[key] for key in ('title', 'author', 'sourcePage', 'license', 'licenseUrl', 'credit')}


def protect_private_path(path, inputs=(), public_root=None):
    target = path.resolve()
    public = (public_root or Path(__file__).resolve().parents[1] / 'public').resolve()
    require(target != public and public not in target.parents, 'Private output must stay outside the public tree')
    require(not any(part.lower().startswith('trailer-before-') or part.lower() == 'archive' for part in target.parts),
            'Preserved archive is read-only')
    for source in inputs:
        if source is not None:
            resolved = source.resolve()
            require(target != resolved and target not in resolved.parents and resolved not in target.parents,
                    'Private output must be separate from all inputs')


def validate_prepared_info(info, duration):
    streams = info['streams']
    require(len(streams) == 1 and streams[0]['codec_type'] == 'audio', 'Prepared music requires one PCM audio stream')
    stream = streams[0]
    require(stream['codec_name'].startswith('pcm_') and stream['sample_rate'] == '48000',
            'Prepared music must be 48kHz PCM')
    samples = Fraction(str(stream['duration_ts'])) * Fraction(stream['time_base']) * 48000
    require(samples.denominator == 1 and samples >= round(duration * 48000),
            'Prepared music is shorter than the chosen duration; looping is forbidden')
    return dict(prepared=True, looped=False, sampleRate=48000, sourceSamples=int(samples),
                consumedSamples=round(duration * 48000))


def prepared_music(path, duration):
    return validate_prepared_info(probe(path), duration)
