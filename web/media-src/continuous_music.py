"""Arrange retained Grok source audio into a sample-indexed continuous bed."""

import argparse
import json
import math
import sys
from array import array
from pathlib import Path

from media_contract import prepare_output, read_timing, require, run, write_json
from music_review import protect_private_path, validate_license
from narration_proof import sha256, verify_character_alignment, verify_words
from opening_audio import RATE, decode_samples, write_float_wav
from soundtrack_proof import (enforce_music_continuity, measure_music_continuity,
                              verify_record)


SCHEMA = "domios.continuous-music.edit-map.v1"
PROGRAM_SAMPLES = 45 * RATE
FINAL_FADE_SAMPLES = round(.2 * RATE)
MINIMUM_JOIN_SAMPLES = round(.35 * RATE)
MINIMUM_DUCK_RAMP_SAMPLES = round(.3 * RATE)
SOURCE_KEY = "originalGrok"
SILENCE_THRESHOLD_DB = "-65"


def audio_record(path):
    return dict(path=str(Path(path).resolve()), sha256=sha256(path))


def decode_source(path):
    decoded = run(["ffmpeg", "-v", "error", "-i", path, "-map", "0:a:0", "-ar", RATE,
                   "-ac", "1", "-c:a", "pcm_f32le", "-f", "f32le", "-"],
                  timeout=120, capture=True)
    values = array("f")
    values.frombytes(decoded)
    if sys.byteorder != "little":
        values.byteswap()
    require(values.itemsize == 4 and len(values) > 0, "Source did not decode to mono 48kHz float PCM")
    require(all(math.isfinite(value) for value in values), "Source contains nonfinite samples")
    return values


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def source_record(scout, source_path, source_hash):
    source = scout.get("sources", {}).get(SOURCE_KEY)
    require(isinstance(source, dict), "Scout lacks the retained original Grok source")
    require(Path(source.get("path", "")).resolve() == Path(source_path).resolve(),
            "Source path differs from the hash-bound scout")
    require(source.get("sha256", "").lower() == source_hash.lower()
            and sha256(source_path).lower() == source_hash.lower(),
            "Source hash differs from the hash-bound scout")
    duration = _finite(source.get("duration"), "Scout source duration")
    require(duration > 0, "Scout source duration must be positive")
    intervals = source.get("lowEnergyIntervals", {}).get(SILENCE_THRESHOLD_DB)
    require(isinstance(intervals, list) and intervals, "Scout lacks measured -65dBFS source intervals")
    return source, intervals


def _integer(value, label):
    require(isinstance(value, int) and not isinstance(value, bool), f"{label} must be a sample index")
    return value


def _finite(value, label):
    require(isinstance(value, (int, float)) and math.isfinite(value), f"{label} must be finite")
    return float(value)


def require_source_hash(edit_map, source_hash):
    value = edit_map.get("sourceSha256")
    require(isinstance(value, str) and len(value) == 64
            and all(character in "0123456789abcdefABCDEF" for character in value),
            "Edit map lacks a valid source SHA256")
    require(value.lower() == source_hash.lower(), "Edit map is bound to a different source hash")


def validate_edit_map(edit_map, source_sample_count, output_sample_count, silent_intervals):
    require(edit_map.get("schema") == SCHEMA, "Unsupported continuous edit-map schema")
    require(edit_map.get("sampleRate") == RATE, "Edit map must use 48kHz sample indices")
    require(edit_map.get("outputSampleCount") == output_sample_count,
            "Edit-map output extent differs from the complete voiced film")
    source_hash = edit_map.get("sourceSha256")
    require(isinstance(source_hash, str) and len(source_hash) == 64
            and all(character in "0123456789abcdefABCDEF" for character in source_hash),
            "Edit map lacks a valid source SHA256")
    segments = edit_map.get("segments")
    require(isinstance(segments, list) and len(segments) >= 4, "Edit map needs enough source cuts to cover the programme")
    ids = set()
    normalized = []
    forbidden = [(round(float(item["start"]) * RATE), round(float(item["end"]) * RATE))
                 for item in silent_intervals]
    for segment in segments:
        segment_id = segment.get("id")
        require(isinstance(segment_id, str) and segment_id and segment_id not in ids,
                "Edit-map segment IDs must be unique")
        ids.add(segment_id)
        source_start = _integer(segment.get("sourceStartSample"), "sourceStartSample")
        source_end = _integer(segment.get("sourceEndSample"), "sourceEndSample")
        output_start = _integer(segment.get("outputStartSample"), "outputStartSample")
        require(0 <= source_start < source_end <= source_sample_count, "Source cut is outside the decoded source")
        require(0 <= output_start < output_sample_count, "Output cut starts outside the programme")
        output_end = output_start + source_end - source_start
        require(output_end <= output_sample_count, "Output cut ends outside the programme")
        for forbidden_start, forbidden_end in forbidden:
            require(source_end <= forbidden_start or source_start >= forbidden_end,
                    f"Source cut {segment_id} overlaps a measured silent interval")
        normalized.append(dict(id=segment_id, sourceStartSample=source_start, sourceEndSample=source_end,
                               outputStartSample=output_start, outputEndSample=output_end))
    normalized.sort(key=lambda segment: segment["outputStartSample"])
    require(normalized[0]["outputStartSample"] == 0 and normalized[-1]["outputEndSample"] == output_sample_count,
            "Edit map must begin at sample zero and cover the final programme sample")
    expected_joins = []
    for index, (left, right) in enumerate(zip(normalized, normalized[1:])):
        require(left["outputStartSample"] < right["outputStartSample"]
                and left["outputEndSample"] < right["outputEndSample"],
                "Edit-map source cuts must progress without nesting")
        start, end = right["outputStartSample"], left["outputEndSample"]
        require(end - start >= MINIMUM_JOIN_SAMPLES,
                f"Adjacent source cuts {left['id']} and {right['id']} need a broad overlapping join")
        if index:
            require(normalized[index - 1]["outputEndSample"] <= start,
                    "Three-way overlapping joins are not supported")
        expected_joins.append(dict(leftSegment=left["id"], rightSegment=right["id"],
                                   startSample=start, endSample=end, curve="equalPower"))
    joins = edit_map.get("joins")
    require(isinstance(joins, list) and joins == expected_joins,
            "Every adjacent source cut needs its exact overlapping equal-power join")
    envelope = edit_map.get("gainEnvelope")
    require(isinstance(envelope, list) and len(envelope) >= 2, "Edit map needs an authored gain envelope")
    previous = -1
    for point in envelope:
        sample = _integer(point.get("sample"), "gainEnvelope.sample")
        gain_db = _finite(point.get("gainDb"), "gainEnvelope.gainDb")
        require(0 <= sample < output_sample_count and previous < sample, "Gain-envelope points must be ordered")
        require(-48 <= gain_db <= 18, "Gain-envelope gain is outside the supported authored range")
        previous = sample
    require(envelope[0]["sample"] == 0 and envelope[-1]["sample"] == output_sample_count - 1,
            "Gain envelope must describe the opening and final programme samples")
    duck = edit_map.get("ducking")
    require(isinstance(duck, dict), "Edit map needs phrase-aware ducking")
    attack = _integer(duck.get("attackSamples"), "ducking.attackSamples")
    release = _integer(duck.get("releaseSamples"), "ducking.releaseSamples")
    reduction = _finite(duck.get("reductionDb"), "ducking.reductionDb")
    require(attack >= MINIMUM_DUCK_RAMP_SAMPLES and release >= MINIMUM_DUCK_RAMP_SAMPLES,
            "Phrase ducking needs broad attack and release ramps")
    require(0 < reduction <= 18, "Phrase ducking reduction must be between 0 and 18dB")
    fade = edit_map.get("finalFade")
    require(fade == dict(startSample=output_sample_count - FINAL_FADE_SAMPLES,
                         endSample=output_sample_count, curve="cosine"),
            "The separately authored final hold fade must be the final 200ms cosine fade")
    return normalized, expected_joins


def phrase_ranges(timing):
    require(len(timing["cues"]) == 13, "Continuous soundtrack requires all thirteen narration phrases")
    ranges = []
    for cue in timing["cues"]:
        words = [word for word in timing["narration"]["words"] if word["cueId"] == cue["id"]]
        require(words, f"Cue {cue['id']} has no aligned voice words")
        start = round(words[0]["finalStart"] * RATE)
        end = round(words[-1]["finalEnd"] * RATE)
        require(0 <= start < end <= PROGRAM_SAMPLES, f"Cue {cue['id']} falls outside the complete programme")
        ranges.append((cue["id"], start, end))
    return ranges


def _base_gain_envelope(points, sample_count):
    gains = array("f", [0]) * sample_count
    point_index = 0
    start_sample = points[0]["sample"]
    start_db = float(points[0]["gainDb"])
    end_sample = points[1]["sample"]
    end_db = float(points[1]["gainDb"])
    for sample in range(sample_count):
        while sample > end_sample and point_index < len(points) - 2:
            point_index += 1
            start_sample = points[point_index]["sample"]
            start_db = float(points[point_index]["gainDb"])
            end_sample = points[point_index + 1]["sample"]
            end_db = float(points[point_index + 1]["gainDb"])
        portion = (sample - start_sample) / (end_sample - start_sample)
        gains[sample] = 10 ** ((start_db + (end_db - start_db) * portion) / 20)
    return gains


def _duck_gain_envelope(timing, duck, sample_count):
    reduction = float(duck["reductionDb"])
    attack = int(duck["attackSamples"])
    release = int(duck["releaseSamples"])
    envelope = array("f", [0]) * sample_count
    for _, phrase_start, phrase_end in phrase_ranges(timing):
        attack_start = max(0, phrase_start - attack)
        attack_length = phrase_start - attack_start
        if attack_length:
            for sample in range(attack_start, phrase_start):
                portion = (sample - attack_start) / attack_length
                value = -reduction * (.5 - .5 * math.cos(math.pi * portion))
                if value < envelope[sample]:
                    envelope[sample] = value
        for sample in range(phrase_start, phrase_end):
            if -reduction < envelope[sample]:
                envelope[sample] = -reduction
        release_end = min(sample_count, phrase_end + release)
        release_length = release_end - phrase_end
        if release_length:
            for sample in range(phrase_end, release_end):
                portion = (sample - phrase_end) / release_length
                value = -reduction * (.5 + .5 * math.cos(math.pi * portion))
                if value < envelope[sample]:
                    envelope[sample] = value
    return envelope


def _join_weights(sample, join):
    length = join["endSample"] - join["startSample"]
    portion = (sample - join["startSample"]) / (length - 1)
    angle = math.pi * .5 * portion
    return math.cos(angle), math.sin(angle)


def _maximum_envelope_slope(points):
    return max(abs(float(right["gainDb"]) - float(left["gainDb"])) * RATE
               / (right["sample"] - left["sample"]) for left, right in zip(points, points[1:]))


def _maximum_sample_slope(values):
    return max((abs(float(values[index]) - float(values[index - 1])) * RATE
                for index in range(1, len(values))), default=0.0)


def render_music(source_samples, timing, edit_map, silent_intervals):
    sample_count = edit_map["outputSampleCount"]
    segments, joins = validate_edit_map(edit_map, len(source_samples), sample_count, silent_intervals)
    phrase_ranges(timing)
    output = array("f", [0]) * sample_count
    coverage = bytearray(sample_count)
    join_by_left = {join["leftSegment"]: join for join in joins}
    join_by_right = {join["rightSegment"]: join for join in joins}
    for segment in segments:
        source_start = segment["sourceStartSample"]
        output_start = segment["outputStartSample"]
        output_end = segment["outputEndSample"]
        incoming = join_by_right.get(segment["id"])
        outgoing = join_by_left.get(segment["id"])
        for sample in range(output_start, output_end):
            source_index = source_start + sample - output_start
            weight = 1.0
            if incoming and sample < incoming["endSample"]:
                _, weight = _join_weights(sample, incoming)
            elif outgoing and sample >= outgoing["startSample"]:
                weight, _ = _join_weights(sample, outgoing)
            output[sample] += float(source_samples[source_index]) * weight
            coverage[sample] += 1
    require(all(count in (1, 2) for count in coverage), "Edit map leaves uncovered samples or has a three-way join")
    gain = _base_gain_envelope(edit_map["gainEnvelope"], sample_count)
    duck_db = _duck_gain_envelope(timing, edit_map["ducking"], sample_count)
    fade_start = edit_map["finalFade"]["startSample"]
    fade_length = edit_map["finalFade"]["endSample"] - fade_start
    for sample in range(sample_count):
        duck = 10 ** (float(duck_db[sample]) / 20)
        fade = 1.0
        if sample >= fade_start:
            portion = (sample - fade_start) / (fade_length - 1)
            fade = .5 + .5 * math.cos(math.pi * portion)
        output[sample] *= gain[sample] * duck * fade
    require(all(math.isfinite(value) for value in output), "Arranged music contains invalid samples")
    return output, dict(segments=segments, joins=joins, phraseRanges=[
        dict(cueId=cue_id, startSample=start, endSample=end) for cue_id, start, end in phrase_ranges(timing)],
        gainEnvelope=dict(points=edit_map["gainEnvelope"],
                          maximumAbsSlopeDbPerSecond=_maximum_envelope_slope(edit_map["gainEnvelope"])),
        ducking=dict(attackSamples=edit_map["ducking"]["attackSamples"],
                     releaseSamples=edit_map["ducking"]["releaseSamples"],
                     reductionDb=edit_map["ducking"]["reductionDb"],
                     maximumAbsSlopeDbPerSecond=_maximum_sample_slope(duck_db)),
        finalFade=dict(startSample=fade_start, endSample=sample_count, durationSeconds=.2,
                       curve="cosine"))


def _validate_prior(prior, timing_path, timing, alignment, narration_audio, source_hash):
    require(prior.get("timingSha256") == sha256(timing_path), "Prior audio proof uses different timing")
    require(prior.get("narration") == timing["narration"], "Prior proof has different word mapping")
    require(prior.get("sourceAudio", {}).get("sha256") == sha256(narration_audio),
            "Prior proof does not use the supplied complete voice recording")
    require(prior.get("sourceAlignment", {}).get("sha256") == sha256(alignment),
            "Prior proof does not use the supplied word alignment")
    require(prior.get("music", {}).get("extractedAudioSha256", "").lower() == source_hash.lower(),
            "Grok source differs from the original soundtrack provenance")
    validate_license(prior["music"])
    verify_record(prior["sourceMusic"], "Prior arranged music")
    require(prior["sourceMusic"]["sha256"].lower() == prior["music"]["sha256"].lower(),
            "Prior arrangement hash differs from its credit")
    verify_record(prior["music"]["arrangement"], "Prior arrangement edit record")
    if prior["music"].get("publicationDecision"):
        verify_record(prior["music"]["publicationDecision"], "Prior publication decision")
    stems = prior.get("mixStems")
    required = {"voicePostDuck", "musicPostDuck", "voiceMastered", "musicMastered", "unmasteredMix"}
    require(isinstance(stems, dict) and set(stems) == required, "Prior proof lacks the complete actual stem set")
    for key in ("voicePostDuck", "voiceMastered"):
        verify_record(stems[key], f"Prior {key}")
    mastering = prior.get("mastering")
    require(isinstance(mastering, dict), "Prior proof lacks fixed mastering")
    gain = mastering.get("linearGain")
    gain_db = mastering.get("gainDb")
    require(isinstance(gain, (int, float)) and math.isfinite(gain) and gain > 0,
            "Prior fixed mastering gain is invalid")
    require(isinstance(gain_db, (int, float)) and math.isfinite(gain_db)
            and abs(gain - 10 ** (gain_db / 20)) <= 1e-12,
            "Prior fixed mastering gain metadata disagrees")
    verify_words(timing)
    verify_character_alignment(timing, alignment)
    return stems, mastering


def arrange(source_path, scout_path, baseline_proof_path, timing_path, edit_map_path, output_dir):
    source_hash = sha256(source_path)
    scout = read_json(scout_path)
    source_record_value, silent_intervals = source_record(scout, source_path, source_hash)
    require(scout.get("method", "").startswith("Read-only hash-bound full-duration mono decode"),
            "Scout is not the retained-source measured envelope")
    timing = read_timing(timing_path)
    require(timing["duration"] == 45, "Continuous soundtrack is bound to the complete 45s voiced film")
    output_sample_count = round(timing["duration"] * RATE)
    require(output_sample_count == PROGRAM_SAMPLES, "Continuous arrangement requires exactly 45s at 48kHz")
    prior = read_json(baseline_proof_path)
    stems, mastering = _validate_prior(prior, timing_path, timing,
                                       Path(prior["sourceAlignment"]["path"]),
                                       Path(prior["sourceAudio"]["path"]), source_hash)
    source_samples = decode_source(source_path)
    require(len(source_samples) == round(float(source_record_value["duration"]) * RATE),
            "Decoded retained source extent differs from the measured scout")
    edit_map = read_json(edit_map_path)
    require_source_hash(edit_map, source_hash)
    segments, joins = validate_edit_map(edit_map, len(source_samples), output_sample_count, silent_intervals)
    expected_voice_count = output_sample_count
    voice_post_duck = decode_samples(Path(stems["voicePostDuck"]["path"]), expected_voice_count)
    voice_mastered = decode_samples(Path(stems["voiceMastered"]["path"]), expected_voice_count)
    require(len(voice_post_duck) == len(voice_mastered) == expected_voice_count,
            "Prior voice stems differ from the complete programme sample count")
    arranged, audit = render_music(source_samples, timing, edit_map, silent_intervals)
    require(sha256(source_path).lower() == source_hash.lower(), "Retained source changed during arrangement")
    output_dir = Path(output_dir)
    inputs = [source_path, scout_path, baseline_proof_path, timing_path, edit_map_path,
              Path(stems["voicePostDuck"]["path"]), Path(stems["voiceMastered"]["path"])]
    protect_private_path(output_dir, inputs)
    prepare_output(output_dir, inputs)
    prepared = output_dir / "prepared-music.wav"
    write_float_wav(prepared, arranged)
    prepared_record = audio_record(prepared)
    measured = measure_music_continuity(arranged, output_sample_count)
    enforce_music_continuity(measured, "Prepared music")
    edit_map_record = dict(path=str(Path(edit_map_path).resolve()), sha256=sha256(edit_map_path))
    baseline_record = dict(path=str(Path(baseline_proof_path).resolve()), sha256=sha256(baseline_proof_path))
    proof = dict(schema="domios.continuous-music.proof.v1",
                 source=str(Path(source_path).resolve()), sourceSha256=source_hash,
                 sourceDurationSamples=len(source_samples), sourceSilentIntervals=[
                     dict(startSample=round(float(item["start"]) * RATE),
                          endSample=round(float(item["end"]) * RATE), thresholdDbfs=-65)
                     for item in silent_intervals],
                 output=str(prepared.resolve()), outputSha256=prepared_record["sha256"],
                 outputSampleCount=output_sample_count, sampleRate=RATE,
                 scout=dict(path=str(Path(scout_path).resolve()), sha256=sha256(scout_path)),
                 editMap=edit_map_record, baselineAudioProof=baseline_record,
                 timingSha256=sha256(timing_path),
                 baselineStems={key: stems[key] for key in ("voicePostDuck", "voiceMastered")},
                 fixedMastering=mastering, editAudit=audit,
                 preparedMusic=prepared_record, continuity=measured,
                 listening="Root source audition and final-film listening remain required; measurements prove dropouts only")
    proof_path = output_dir / "continuous-proof.json"
    write_json(proof_path, proof)
    proof_record = audio_record(proof_path)
    previous_credit = dict(prior["music"])
    previous_arrangement = previous_credit.get("arrangement")
    previous_credit["previousArrangement"] = previous_arrangement
    previous_credit["sha256"] = prepared_record["sha256"]
    previous_credit["extractedAudioSha256"] = source_hash
    previous_credit["arrangement"] = dict(path=str(proof_path.resolve()), sha256=proof_record["sha256"])
    previous_credit["credit"] = (
        "Music created with Grok (SpaceXAI) from retained source audio, arranged and mixed for Domios."
    )
    previous_credit["modificationDisclosure"] = (
        "A new continuous 45-second arrangement made only from sample-indexed cuts, overlapping joins, "
        "an authored gain envelope, and phrase-aware ducking. No source generation, pitch shift, or time stretch."
    )
    provenance_path = output_dir / "provenance.json"
    provenance = dict(previous_credit, source=dict(path=str(Path(source_path).resolve()), sha256=source_hash),
                     originalMediaSha256=prior["music"]["originalMediaSha256"],
                     oldArrangement=previous_arrangement,
                     continuousProof=dict(path=str(proof_path.resolve()), sha256=proof_record["sha256"]),
                     editMap=edit_map_record,
                     fixedMastering=dict(gainDb=mastering["gainDb"], linearGain=mastering["linearGain"]))
    write_json(provenance_path, provenance)
    duplicate_stem = output_dir / "music-post-duck.wav"
    duplicate_stem.write_bytes(prepared.read_bytes())
    return dict(preparedMusic=prepared_record, continuousProof=audio_record(proof_path),
                provenance=audio_record(provenance_path), musicPostDuck=audio_record(duplicate_stem),
                sourceSha256=source_hash, editMapSha256=edit_map_record["sha256"],
                baselineAudioProofSha256=baseline_record["sha256"], segmentCount=len(segments),
                joinCount=len(joins))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source", "scout", "baseline-audio-proof", "timing", "edit-map", "output-dir"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    result = arrange(args.source, args.scout, args.baseline_audio_proof, args.timing, args.edit_map, args.output_dir)
    print(json.dumps(result, indent=2))
