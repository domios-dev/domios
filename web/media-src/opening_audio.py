import math
import struct
import sys
import wave
from array import array
from pathlib import Path

from media_contract import require, run


RATE = 48000
MINIMUM_SEPARATION_DB = 14
ONSET_WINDOWS = ((round(.1 * RATE), -50), (round(.65 * RATE), -40))


def decode_samples(path, count):
    decoded = run(['ffmpeg', '-v', 'error', '-i', path, '-map', '0:a:0', '-ar', RATE, '-ac', '1',
                   '-c:a', 'pcm_f32le', '-f', 'f32le', '-'], timeout=120, capture=True)
    values = array('f')
    values.frombytes(decoded)
    if sys.byteorder != 'little':
        values.byteswap()
    require(values.itemsize == 4 and len(values) == count, f'Expected {count} samples from {path}')
    require(all(math.isfinite(value) for value in values), f'Nonfinite audio sample: {path}')
    return values


def decode_excerpt(path, start, duration, count):
    filters = (f'aresample={RATE},aformat=sample_fmts=flt:channel_layouts=mono,'
               f'atrim=start={start:.9f}:duration={duration:.9f},asetpts=PTS-STARTPTS')
    decoded = run(['ffmpeg', '-v', 'error', '-i', path, '-map', '0:a:0', '-af', filters,
                   '-ar', RATE, '-ac', '1', '-c:a', 'pcm_f32le', '-f', 'f32le', '-'],
                  timeout=120, capture=True)
    values = array('f')
    values.frombytes(decoded)
    if sys.byteorder != 'little':
        values.byteswap()
    require(values.itemsize == 4 and len(values) == count,
            f'Opening excerpt is shorter than its requested {duration:.3f}s selection')
    require(all(math.isfinite(value) for value in values), 'Nonfinite opening excerpt sample')
    return values


def rms(values, start=0, end=None):
    end = len(values) if end is None else end
    require(0 <= start < end <= len(values), 'Invalid RMS sample range')
    return math.sqrt(math.fsum(float(values[index]) ** 2 for index in range(start, end)) / (end - start))


def dbfs(value):
    return 20 * math.log10(value) if value > 0 else float('-inf')


def envelope(length, ramp_samples, crossfade_start, end_sample):
    require(0 < ramp_samples < crossfade_start < end_sample == length, 'Invalid opening shape bounds')
    values = array('f', [0]) * length
    for index in range(length):
        ramp = 1.0 if index >= ramp_samples else .5 - .5 * math.cos(math.pi * index / ramp_samples)
        fade = 1.0 if index < crossfade_start else .5 + .5 * math.cos(
            math.pi * (index - crossfade_start) / (end_sample - crossfade_start))
        values[index] = ramp * fade
    return values


def _coefficients(base, addition, start, end):
    inverse = 1 / (end - start)
    base_power = math.fsum(float(base[index]) ** 2 for index in range(start, end)) * inverse
    cross_power = math.fsum(float(base[index]) * (float(addition[index]) if index < len(addition) else 0)
                            for index in range(start, end)) * inverse
    added_power = math.fsum((float(addition[index]) if index < len(addition) else 0) ** 2
                            for index in range(start, end)) * inverse
    return base_power, cross_power, added_power


def _upper_gain(base, addition, start, end, limit):
    power, cross, added = _coefficients(base, addition, start, end)
    if added == 0:
        return float('inf') if power <= limit * limit else -1
    if power > limit * limit:
        return -1
    discriminant = cross * cross - added * (power - limit * limit)
    require(discriminant >= 0, 'Opening level constraint has no real gain boundary')
    return (-cross + math.sqrt(discriminant)) / added


def _lower_gain(base, addition, start, end, limit):
    power, cross, added = _coefficients(base, addition, start, end)
    if power > limit * limit:
        return 0.0
    require(added > 0, 'Opening source has no measurable energy in an onset window')
    discriminant = cross * cross - added * (power - limit * limit)
    require(discriminant >= 0, 'Opening onset constraint has no real gain boundary')
    return (-cross + math.sqrt(discriminant)) / added


def choose_gain(base_music, excerpt_term, voice, timing, opening_end, count):
    onset_minimum = 0.0
    for window_samples, floor_db in ONSET_WINDOWS:
        end = min(window_samples, count)
        floor = 10 ** (floor_db / 20)
        source_level = rms(excerpt_term, 0, end)
        require(source_level > 0, 'Opening source has no measurable material at the beginning')
        onset_minimum = max(onset_minimum, _lower_gain(base_music, excerpt_term, 0, end, floor))
        onset_minimum = max(onset_minimum, floor / source_level)

    separation_maximum = float('inf')
    threshold_ratio = 10 ** (MINIMUM_SEPARATION_DB / 20)
    for cue in timing['cues']:
        words = [word for word in timing['narration']['words'] if word['cueId'] == cue['id']]
        require(words, f'Cue {cue["id"]} has no aligned phrase')
        voice_start = round(words[0]['finalStart'] * RATE)
        voice_end = round(words[-1]['finalEnd'] * RATE)
        start = max(0, voice_start)
        end = min(count, voice_end)
        if start >= end or start >= round(opening_end * RATE):
            continue
        voice_level = rms(voice, voice_start, voice_end)
        limit = voice_level / threshold_ratio
        separation_maximum = min(separation_maximum, _upper_gain(base_music, excerpt_term, start, end, limit))

    require(separation_maximum >= 0, 'Existing opening music already violates phrase separation')
    gain = onset_minimum * 1.35 + 1e-8
    require(gain < separation_maximum * .9,
            f'Opening onset floors require gain {gain:.6g}, but phrase separation allows {separation_maximum:.6g}')
    return gain, dict(onsetMinimumLinearGain=onset_minimum, phraseMaximumLinearGain=separation_maximum,
                      selectedLinearGain=gain, selectedGainDb=20 * math.log10(gain))


def assemble_music(base_music, excerpt, gain, end_sample, crossfade_start, ramp_samples):
    require(len(base_music) >= end_sample and len(excerpt) == end_sample, 'Opening source and mastered music extents differ')
    result = array('f', base_music)
    term = shape_excerpt(excerpt, crossfade_start, ramp_samples)
    for index in range(end_sample):
        result[index] = float(base_music[index]) + gain * term[index]
    require(all(math.isfinite(value) and abs(value) <= 1 for value in result), 'Opening mix clips or contains invalid samples')
    return result, term


def shape_excerpt(excerpt, crossfade_start, ramp_samples):
    shape = envelope(len(excerpt), ramp_samples, crossfade_start, len(excerpt))
    return array('f', (float(excerpt[index]) * shape[index] for index in range(len(excerpt))))


def verify_preservation(original_voice, candidate_voice, original_music, candidate_music, opening_end_sample):
    require(len(original_voice) == len(candidate_voice), 'Voice sample count changed')
    require(len(original_music) == len(candidate_music), 'Music sample count changed')
    require(all(a == b for a, b in zip(original_voice, candidate_voice)), 'Baseline mastered voice PCM changed')
    require(all(a == b for a, b in zip(original_music[opening_end_sample:], candidate_music[opening_end_sample:])),
            'Mastered music PCM after the bounded opening changed')


def write_float_wav(path, values):
    samples = array('f', values)
    if sys.byteorder != 'little':
        samples.byteswap()
    data = samples.tobytes()
    header = (b'RIFF' + struct.pack('<I', 36 + len(data)) + b'WAVE' + b'fmt ' + struct.pack(
        '<IHHIIHH', 16, 3, 1, RATE, RATE * 4, 4, 32) + b'data' + struct.pack('<I', len(data)))
    Path(path).write_bytes(header + data)


def write_pcm16_wav(path, values):
    data = pcm16_data(values)
    write_pcm16_data(path, data, len(values))


def write_pcm16_wav_preserving_tail(path, values, reference_path, start_sample):
    require(0 <= start_sample < len(values), 'Final PCM tail boundary is outside the sample range')
    reference_tail = pcm16_tail(reference_path, start_sample, len(values))
    data = bytearray(pcm16_data(values))
    data[start_sample * 2:] = reference_tail
    write_pcm16_data(path, data, len(values))
    verify_pcm16_tail_preserved(reference_path, path, start_sample, len(values))


def verify_pcm16_tail_preserved(reference_path, candidate_path, start_sample, total_samples):
    reference = pcm16_tail(reference_path, start_sample, total_samples)
    candidate = pcm16_tail(candidate_path, start_sample, total_samples)
    require(candidate == reference, 'Final PCM16 tail after the opening boundary changed')


def pcm16_tail(path, start_sample, total_samples):
    require(0 <= start_sample < total_samples, 'Final PCM tail boundary is outside the sample range')
    with wave.open(str(path), 'rb') as source:
        require(source.getnchannels() == 1 and source.getsampwidth() == 2 and source.getframerate() == RATE
                and source.getnframes() == total_samples and source.getcomptype() == 'NONE',
                f'Expected mono PCM16 at {RATE} Hz with {total_samples} samples: {path}')
        source.setpos(start_sample)
        tail = source.readframes(total_samples - start_sample)
    require(len(tail) == (total_samples - start_sample) * 2, f'Incomplete PCM16 tail: {path}')
    return tail


def pcm16_data(values):
    converted = array('h', (max(-32768, min(32767, round(float(value) * 32768))) for value in values))
    if sys.byteorder != 'little':
        converted.byteswap()
    return converted.tobytes()


def write_pcm16_data(path, data, sample_count):
    require(len(data) == sample_count * 2, 'PCM16 data size differs from its sample count')
    header = (b'RIFF' + struct.pack('<I', 36 + len(data)) + b'WAVE' + b'fmt ' + struct.pack(
        '<IHHIIHH', 16, 1, 1, RATE, RATE * 2, 2, 16) + b'data' + struct.pack('<I', len(data)))
    Path(path).write_bytes(header + bytes(data))


def sum_samples(first, second):
    require(len(first) == len(second), 'Mastered stems have different sample extents')
    result = array('f', (float(a) + float(b) for a, b in zip(first, second)))
    require(all(math.isfinite(value) and abs(value) <= 1 for value in result), 'Mastered stem sum clips')
    return result
