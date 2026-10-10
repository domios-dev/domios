import copy
import json
import math
import re
import subprocess
from array import array
from pathlib import Path

from PIL import Image, ImageDraw

from media_contract import bounded_process, prepare_output, read_timing, require, run, write_json
from narration_proof import sha256, verify_character_alignment, verify_words
from opening_audio import decode_samples, write_float_wav
from trailer_scenes import SIZE, WHITE, BLUE, ending_mesh, face, wallpaper, window

RATE = 48000
INSERT_FRAME = 1112
INSERT_FRAMES = 280
INSERT_SAMPLES = INSERT_FRAMES * RATE // 30
MEMORY_LEAD_SAMPLES = 5760


def load(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def record(path):
    return dict(path=str(path.resolve()), sha256=sha256(path))


def decode_all(path, output):
    run(['ffmpeg', '-v', 'error', '-nostdin', '-n', '-i', path, '-vn', '-ar', RATE,
         '-ac', '1', '-c:a', 'pcm_f32le', output])
    from media_contract import probe
    count = round(float(probe(output)['format']['duration']) * RATE)
    return decode_samples(output, count)


def prepare(original_timing, original_audio, original_alignment, memory_audio,
            memory_alignment, memory_metadata, music, music_credit, output):
    prepare_output(output, [original_audio, memory_audio, music])
    old = read_timing(original_timing)
    verify_words(old)
    verify_character_alignment(old, original_alignment)
    require(sha256(original_audio) == old['narration']['audioSha256'], 'Original voice changed')
    addition = load(memory_metadata)
    require(sha256(memory_audio) == addition['sha256'], 'Additional Memory voice changed')
    require(sha256(memory_alignment) == addition['alignmentSha256'], 'Memory alignment changed')
    require(addition['voice'] == old['narration']['voice'] and addition['model'] == old['narration']['model'],
            'Memory narration must use the existing voice and model')
    cut = INSERT_FRAME / 30 - old['narration']['offset']
    cut_sample = round(cut * RATE)
    original = decode_all(original_audio, output / 'original-decoded.wav')
    memory = decode_all(memory_audio, output / 'memory-decoded.wav')
    require(MEMORY_LEAD_SAMPLES + len(memory) <= INSERT_SAMPLES, 'Memory speech exceeds its episode')
    slot = array('f', [0]) * INSERT_SAMPLES
    slot[MEMORY_LEAD_SAMPLES:MEMORY_LEAD_SAMPLES + len(memory)] = memory
    combined = original[:cut_sample] + slot + original[cut_sample:]
    voice = output / 'narration-extended.wav'
    write_float_wav(voice, combined)
    require(combined[:cut_sample] == original[:cut_sample]
            and combined[cut_sample + INSERT_SAMPLES:] == original[cut_sample:], 'Original voice PCM changed')
    base_alignment = load(original_alignment)['alignment']
    extra_alignment = load(memory_alignment)['alignment']
    old_text = ''.join(base_alignment['characters'])
    extra_text = ''.join(extra_alignment['characters'])
    require(extra_text == addition['text'], 'Memory text differs from service timestamps')
    split = old_text.index(old['cues'][-1]['text'])
    spoken_prefix = [index for index in range(split) if not base_alignment['characters'][index].isspace()]
    require(max(base_alignment['character_end_times_seconds'][index] for index in spoken_prefix) <= cut
            and base_alignment['character_start_times_seconds'][split] >= cut,
            'Original speech crosses the insertion boundary')
    duration_added = INSERT_FRAMES / 30
    alignment = {}
    alignment['characters'] = base_alignment['characters'][:split] + extra_alignment['characters'] + ['\n', '\n'] + base_alignment['characters'][split:]
    for key in ('character_start_times_seconds', 'character_end_times_seconds'):
        times = [time + cut + MEMORY_LEAD_SAMPLES / RATE for time in extra_alignment[key]]
        prefix = [min(time, cut) if base_alignment['characters'][index].isspace() else time
                  for index, time in enumerate(base_alignment[key][:split])]
        alignment[key] = prefix + times + [times[-1], times[-1]] + [time + duration_added for time in base_alignment[key][split:]]
    alignment_path = output / 'alignment.json'
    lineage = dict(originalAudio=record(original_audio), originalAlignment=record(original_alignment),
                   memoryAudio=record(memory_audio), memoryAlignment=record(memory_alignment),
                   insertSourceSample=cut_sample, insertedSamples=INSERT_SAMPLES,
                   memoryLeadSamples=MEMORY_LEAD_SAMPLES, originalPcmPrefixAndTailExact=True,
                   whitespaceTiming='Nonspoken separator timestamps clipped at insertion; every spoken timestamp retained')
    write_json(alignment_path, dict(alignment=alignment, assembly=lineage))
    timing = copy.deepcopy(old)
    timing['duration'] += duration_added
    timing['chapters'][-1]['end'] = timing['duration']
    final = timing['cues'].pop()
    for key in ('start', 'end', 'visualStart', 'visualEnd'):
        final[key] += duration_added
    final['visualStart'] = (INSERT_FRAME + INSERT_FRAMES) / 30
    final['visualEnd'] = timing['duration']
    final['id'] = 14
    offset = timing['narration']['offset']
    timing['cues'].append(dict(id=13, text=extra_text,
                              start=cut + MEMORY_LEAD_SAMPLES / RATE + extra_alignment['character_start_times_seconds'][0] + offset,
                              end=cut + MEMORY_LEAD_SAMPLES / RATE + extra_alignment['character_end_times_seconds'][-1] + offset,
                              visualStart=timing['cues'][-1]['visualEnd'],
                              visualEnd=(INSERT_FRAME + INSERT_FRAMES) / 30))
    timing['cues'].append(final)
    narration = timing['narration']
    narration.update(audioSha256=sha256(voice), alignmentSha256=sha256(alignment_path),
                     sourceDuration=len(combined) / RATE,
                     requestsTotal=narration['requestsTotal'] + addition['requests'],
                     charactersTotal=narration['charactersTotal'] + addition['characters'],
                     method='Original service-timestamped take plus owner-requested Memory take; sample-indexed insertion with original PCM prefix and tail preserved',
                     assembly=lineage)
    words = []
    position = 0
    text = ''.join(alignment['characters'])
    for cue in timing['cues']:
        while position < len(text) and text[position].isspace():
            position += 1
        require(text.startswith(cue['text'], position), 'Assembled phrase text mismatch')
        for match in re.finditer(r'\S+', cue['text']):
            first, last = position + match.start(), position + match.end() - 1
            start = alignment['character_start_times_seconds'][first]
            end = alignment['character_end_times_seconds'][last]
            words.append(dict(cueId=cue['id'], text=match.group(), sourceStart=start, sourceEnd=end,
                              finalStart=start + offset, finalEnd=end + offset, speed=1))
        position += len(cue['text'])
    require(not text[position:].strip(), 'Unmapped assembled text')
    narration['words'] = words
    timing_path = output / 'timing.json'
    write_json(timing_path, timing)
    read_timing(timing_path)
    verify_words(timing)
    verify_character_alignment(timing, alignment_path)
    extend_bed(music, music_credit, output, round(timing['duration'] * RATE))
    write_json(output / 'voice-assembly.json', lineage)


def extend_bed(music, credit_path, output, expected_count):
    credit = load(credit_path)
    require(sha256(music) == credit['sha256'].lower(), 'Prior continuous arrangement changed')
    original = decode_samples(music, 45 * RATE)
    cut = INSERT_FRAME * RATE // 30
    fade = round(.35 * RATE)
    donor_start = 7 * RATE
    donor_count = INSERT_SAMPLES + 2 * fade
    donor = original[donor_start:donor_start + donor_count]
    prefix, tail = original[:cut + fade], original[cut - fade:]
    require(len(donor) == donor_count, 'Music extension exceeds retained source')
    joined = array('f', prefix[:-2 * fade])
    for index in range(2 * fade):
        weight = .5 - .5 * math.cos(math.pi * index / (2 * fade - 1))
        joined.append(prefix[-2 * fade + index] * (1 - weight) + donor[index] * weight)
    joined.extend(donor[2 * fade:-2 * fade])
    for index in range(2 * fade):
        weight = .5 - .5 * math.cos(math.pi * index / (2 * fade - 1))
        joined.append(donor[-2 * fade + index] * (1 - weight) + tail[index] * weight)
    joined.extend(tail[2 * fade:])
    require(len(joined) == expected_count, 'Music extension sample extent mismatch')
    target = output / 'prepared-music.wav'
    write_float_wav(target, joined)
    arrangement = dict(previousArrangement=record(music), donorStartSample=donor_start,
                       donorSamples=donor_count, insertionSample=cut, addedSamples=INSERT_SAMPLES,
                       joinSamples=2 * fade, curve='complementary raised cosine',
                       output=record(target), outputSamples=len(joined))
    write_json(output / 'music-extension.json', arrangement)
    credit['sha256'] = sha256(target)
    credit['previousArrangement'] = credit.get('arrangement')
    credit['arrangement'] = record(output / 'music-extension.json')
    credit.pop('continuousProof', None)
    credit.pop('editMap', None)
    credit['modificationDisclosure'] += ' Extended for the spoken Memory episode using the same retained continuous arrangement and two cosine joins.'
    credit['credit'] += ' Arrangement extended for the Memory episode.'
    write_json(output / 'provenance.json', credit)


def computer_finale(font, brand):
    image = wallpaper().copy()
    draw = ImageDraw.Draw(image)
    draw.ellipse((170, 610, 820, 651), fill='#111b29')
    draw.rounded_rectangle((395, 500, 475, 624), 12, fill='#6b7d90')
    draw.polygon([(380, 602), (490, 602), (553, 639), (317, 639)], fill='#91a0af')
    draw.rounded_rectangle((45, 81, 856, 575), 22, fill='#4e6074')
    draw.rounded_rectangle((48, 84, 853, 572), 20, fill='#1d2937')
    content = ending_mesh().crop((334, 50, 3496, 2079))
    content.thumbnail((783, 448), Image.Resampling.LANCZOS)
    screen = Image.new('RGB', (783, 448), '#10161e')
    screen.paste(content, ((783 - content.width) // 2, (448 - content.height) // 2))
    image.paste(screen, (59, 98))
    draw.ellipse((447, 553, 453, 559), fill=BLUE)
    mark = brand.resize((64, 64), Image.Resampling.LANCZOS)
    image.paste(mark, (930, 210), mark)
    draw.multiline_text((930, 292), 'Your next idea.\nA whole team\nbehind you.', font=face(font, 34), fill=WHITE, spacing=4)
    draw.text((930, 514), 'domios.dev', font=face(font, 20), fill=BLUE, anchor='lm')
    return image


def memory_picture(frame, font, brand):
    image = wallpaper().copy()
    image.paste(window(frame, 790, 522, font), (64, 112))
    mark = brand.resize((64, 64), Image.Resampling.LANCZOS)
    image.paste(mark, (930, 210), mark)
    draw = ImageDraw.Draw(image)
    draw.multiline_text((930, 292), 'One memory.\nEvery agent.', font=face(font, 32), fill=WHITE, spacing=4)
    draw.text((930, 448), 'Shared on this machine.', font=face(font, 14), fill=WHITE)
    draw.text((930, 514), 'domios.dev', font=face(font, 20), fill=BLUE, anchor='lm')
    return image


def decoded_frames(path, count, start=0, size=SIZE):
    command = ['ffmpeg', '-v', 'error', '-nostdin', '-ss', str(start), '-i', path,
               '-vf', f'scale={size[0]}:{size[1]}:force_original_aspect_ratio=decrease,pad={size[0]}:{size[1]}:(ow-iw)/2:(oh-ih)/2',
               '-frames:v', count, '-fps_mode', 'passthrough', '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-']
    with bounded_process(command, 300, stdout=subprocess.PIPE) as process:
        try:
            for index in range(count):
                data = process.stdout.read(math.prod(size) * 3)
                require(len(data) == math.prod(size) * 3, f'Incomplete native frame {index}')
                yield Image.frombytes('RGB', size, data)
        finally:
            while process.stdout.read(math.prod(size) * 3):
                pass
            require(process.wait(timeout=30) == 0, 'Native frame decode failed')
