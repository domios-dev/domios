import ast
import math

from media_contract import read_timing, require, run
from trailer_scenes import FPS, SOURCE_SIZE, camera_at, camera_velocity, crop_at, source_time, timed_shots, cuts_for


def baseline_shots(reference):
    source = run(['git', 'show', f'{reference}:web/media-src/trailer_scenes.py'], timeout=30, capture=True)
    tree = ast.parse(source.decode('utf-8'))
    assignment = next(node for node in tree.body if isinstance(node, ast.Assign)
                      and any(isinstance(target, ast.Name) and target.id == 'SHOTS' for target in node.targets))
    keys = ['name', 'at', 'duration', 'source', 'start', 'end', 'camera', 'target', 'title', 'scene']
    return [dict({'title': '', 'scene': 'footage'}, **dict(zip(keys, map(ast.literal_eval, call.args))))
            for call in assignment.value.elts]


def old_source_time(edit, time):
    low, high = 0, 1
    for _ in range(60):
        u = (low + high) / 2
        if edit['duration'] * (u - .10 * math.sin(math.pi * u)) < time:
            low = u
        else:
            high = u
    return edit['start'] + (edit['end'] - edit['start']) * (low + high) / 2


def old_camera(edit, time):
    u = min(1, max(0, time / (edit['duration'] - 1 / 24)))
    t = u ** 3 * (u * (u * 6 - 15) + 10)
    x, y, width = [a + (b - a) * t for a, b in zip(edit['camera'], edit['target'])]
    return (x, min(y, SOURCE_SIZE[1] - width * 9 / 16), width)


def baseline_evidence(reference):
    edits = baseline_shots(reference)
    invalid = []
    for edit in edits:
        if edit['scene'] != 'footage':
            continue
        for point in ['camera', 'target']:
            x, y, width = edit[point]
            if x < 0 or y < 0 or x + width > SOURCE_SIZE[0] or y + width * 9 / 16 > SOURCE_SIZE[1]:
                invalid.append(dict(shot=edit['name'], point=point, authored=list(edit[point]),
                                    maximumY=SOURCE_SIZE[1] - width * 9 / 16))
    left = next(edit for edit in edits if edit['at'] == 8)
    right = next(edit for edit in edits if edit['at'] == 11)
    before = (left['end'] - left['start']) / left['duration'] / (1 + .1 * math.pi)
    after = (right['end'] - right['start']) / right['duration'] / (1 - .1 * math.pi)
    samples = []
    for index in range(1080):
        time = index / 24
        edit = next(edit for edit in edits if edit['at'] <= time < edit['at'] + edit['duration'])
        local = time - edit['at']
        samples.append(dict(frame=index, outputTime=time, shot=edit['name'], source=edit['source'],
                            sourceTime=old_source_time(edit, local) if edit['scene'] == 'footage' else edit['start'],
                            actualCamera=list(old_camera(edit, local)) if edit['scene'] == 'footage' else None))
    return dict(reference=reference, fps=24, frameCount=1080, invalidAuthoredCrops=invalid,
                board11=dict(sourceTime=left['end'], speedBefore=before, speedAfter=after,
                             speedRatio=after / before, cameraStopsAtJoin=True), samples=samples)


def timeline_audit(baseline_ref=None, review_range=None, timing=None):
    timing = timing or read_timing()
    shots = timed_shots(timing)
    cuts = cuts_for(shots)
    require(FPS == 30, 'Candidate must use native30fps')
    require(shots[0]['at'] == 0 and abs(shots[-1]['at'] + shots[-1]['duration'] - timing['duration']) < 1e-8, 'Duration mismatch')
    require(all(abs(a['at'] + a['duration'] - b['at']) < 1e-8 for a, b in zip(shots, shots[1:])), 'Timeline gap')
    samples = []
    for edit in shots:
        if edit['scene'] == 'footage':
            crop_at(edit, edit['duration'])
        count = round(edit['duration'] * FPS)
        previous = None
        for index in range(count):
            local = index / FPS
            output = edit['at'] + local
            source = source_time(edit, local)
            require(previous is None or source >= previous, 'Source clock reversed')
            previous = source
            samples.append(dict(frame=round(output * FPS), outputTime=output, shot=edit['name'],
                                source=edit['source'], sourceTime=source,
                                decodedSourceFrame=math.floor(source * FPS + 1e-7),
                                decodedSourceTime=math.floor(source * FPS + 1e-7) / FPS,
                                crop=list(crop_at(edit, local)) if edit['scene'] == 'footage' else None))
    left = next(edit for edit in shots if edit['name'] == 'A team takes the work')
    right = next(edit for edit in shots if edit['name'] == 'Across the board')
    normalized = (SOURCE_SIZE[0], SOURCE_SIZE[1], SOURCE_SIZE[0])
    join = {}
    for key, fn in [('position', camera_at), ('velocity', camera_velocity)]:
        a = [value / size for value, size in zip(fn(left, left['duration']), normalized)]
        b = [value / size for value, size in zip(fn(right, 0), normalized)]
        require(max(abs(x - y) for x, y in zip(a, b)) <= 1e-6, f'Board join {key} discontinuity')
        join[key] = dict(before=a, after=b)
    require(any(abs(v) > 1e-6 for v in join['velocity']['before']), 'Board camera rests at cue join')
    require(abs(source_time(left, left['duration']) - source_time(right, 0)) < 1e-8, 'Board source discontinuity')
    before_speed = (left['end'] - left['start']) / left['duration']
    after_speed = (right['end'] - right['start']) / right['duration']
    require(abs(before_speed - after_speed) < 1e-6, 'Board source speed resets at cue join')
    join.update(outputTime=right['at'], sourceTime=right['start'], sourceSpeedBefore=before_speed, sourceSpeedAfter=after_speed)
    require(len(samples) == round(timing['duration'] * FPS), 'Frame count differs from timing')
    require([sample['frame'] for sample in samples] == list(range(len(samples))), 'Missing/duplicate output frame in edit map')
    for a, b in zip(shots, shots[1:]):
        if b['at'] not in cuts:
            require(a['source'] == b['source'] and abs(a['end'] - b['start']) < 1e-8, 'Undeclared source cut')
    return dict(duration=timing['duration'], fps=FPS, frameCount=len(samples), sourceSize=SOURCE_SIZE,
                boardJoin=join, hardCuts=cuts, reviewRange=review_range,
                samples=[sample for sample in samples if review_range is None
                         or review_range[0] <= sample['outputTime'] < review_range[1]],
                baseline=baseline_evidence(baseline_ref) if baseline_ref else None,
                evidence='Analytic/sample source audit; actual A/B visual symptom review remains required')
