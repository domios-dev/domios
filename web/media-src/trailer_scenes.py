import hashlib
import json
import math
import subprocess
from functools import lru_cache
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont
from media_contract import bounded_process, require
from ending_memory import MEMORY_END_FRAME


FPS = 30
SIZE = (1280, 720)
SOURCE_SIZE = (2048, 1396)
BLUE = '#58a6ff'
INK = '#0d1117'
WHITE = '#f0f6fc'
OPENING_FRAMES = 48
ENDING_START_FRAME = round(37.0666666667 * FPS)
ENDING_END_FRAME = 45 * FPS
BRAND_DURATION = 1.6
OPENING_FADE_START_FRAME = 40
ENDING_MESH_PATH = Path(__file__).resolve().parent / 'assets/owner-mesh.png'
ENDING_MESH_SHA256 = '204446b6fd91bfa1925f4c0f4df09a10b994c46393b82e0f3b6f1ae016d82871'


def shot(name, at, duration, source, start, end, camera, target, title='', scene='footage'):
    return dict(name=name, at=at, duration=duration, source=source, start=start, end=end,
                camera=camera, target=target, title=title, scene=scene)


SHOTS = [
    shot('Your desktop', 0, 4, 'mesh-talk', 16, 16.1, (0, 0, 1), (0, 0, 1), 'A home for agents', 'desktop'),
    shot('Agents in tabs', 4, 4, 'mesh-talk', 16, 20, (5, 50, 2040), (5, 100, 1750)),
    shot('A team takes the work', 8, 3, 'board', .2, 3.2, (350, 300, 1680), (350, 310, 1160), 'Give work a team'),
    shot('Across the board', 11, 4, 'board', 3.2, 7.2, (350, 310, 1160), (850, 310, 1160)),
    shot('Your machines', 15, 4, 'mesh', 0, .1, (0, 0, 1), (0, 0, 1), 'Every machine. Connected.', 'network'),
    shot('Open a peer', 19, 4, 'mesh', 0, 4, (350, 140, 1680), (620, 400, 1160)),
    shot('Request setup', 23, 2, 'mesh-talk', 33, 35, (460, 185, 1470), (460, 185, 1470)),
    shot('Dispatch to peers', 25, 2, 'mesh-talk', 46, 48, (460, 245, 1470), (460, 245, 1470)),
    shot('Reports arrive', 27, 3, 'mesh-talk', 95, 98, (460, 300, 1470), (490, 550, 1390)),
    shot('Choose providers', 30, 3, 'plugins', 1.5, 5.6, (350, 80, 1680), (365, 200, 1320)),
    shot('Choose tools', 33, 2.5, 'plugins', 7.3, 8.7, (910, 100, 1100), (930, 190, 1070)),
    shot('Add capabilities', 35.5, 2.5, 'plugins', 11, 14.9, (900, 120, 1110), (920, 220, 1090)),
    shot('Fleet summary', 38, 3, 'mesh-talk', 139, 146, (460, 570, 1450), (490, 620, 1350)),
    shot('Threshold', 41, 4, 'mesh', 0, .1, (0, 0, 1), (0, 0, 1), scene='end'),
]

HARD_CUTS = {4: 'Desktop establishing view to recorded app; same source frame16',
             8: 'Conversation to team board', 15: 'Board to mesh establishing view',
             19: 'Illustrated connections to recorded peer action',
             23: 'Peer workspace to request setup', 25: 'Omit waiting between setup and dispatch within the request beat',
             27: 'Dispatch to returned reports; omit asynchronous wait',
             30: 'Reports to providers', 33: 'Providers to Git tool',
             35.5: 'Git tool to plugin documentation', 38: 'Plugins to fleet summary',
             41: 'Fleet payoff to brand close'}


def timed_shots(timing):
    groups = [[0], [1], [2], [3], [4], [5], [6, 7], [8], [9], [10], [11], [12], [13]]
    edits = []
    cues = timing['cues']
    require(len(cues) in (13, 14), 'Unsupported picture episode count')
    indexed_cues = cues if len(cues) == 13 else cues[:12] + cues[13:]
    for cue, indices in zip(indexed_cues, groups):
        total = cue['visualEnd'] - cue['visualStart']
        for part, index in enumerate(indices):
            edit = SHOTS[index].copy()
            at = cue['visualStart'] + round(part * total / len(indices) * FPS) / FPS
            end = cue['visualEnd'] if part == len(indices) - 1 else cue['visualStart'] + round(total / len(indices) * FPS) / FPS
            edit.update(at=at, duration=end - at, cueId=cue['id'])
            if index in (1, 5, 6, 7):
                edit['end'] = edit['start'] + min(edit['duration'], 4 if index in (1, 5) else 2)
            edits.append(edit)
    if len(cues) == 14:
        cue = cues[12]
        edits.insert(-1, shot('Shared Memory', cue['visualStart'], cue['visualEnd'] - cue['visualStart'],
                              'memory', 6.2, 6.2 + cue['visualEnd'] - cue['visualStart'],
                              (0, 0, 1), (0, 0, 1), scene='memory'))
        edits[-2]['cueId'] = cue['id']
    board_at = timing['cues'][2]['visualStart']
    board_duration = timing['cues'][3]['visualEnd'] - board_at
    for edit in edits:
        if edit['source'] == 'board':
            edit.update(groupAt=board_at, groupDuration=board_duration,
                        start=.2 + 7 * (edit['at'] - board_at) / board_duration,
                        end=.2 + 7 * (edit['at'] + edit['duration'] - board_at) / board_duration)
    return edits


def cuts_for(edits):
    reasons = list(HARD_CUTS.values())
    original = [edit for edit in edits[1:] if edit['name'] not in ('Across the board', 'Shared Memory')]
    cuts = {edit['at']: reason for edit, reason in zip(original, reasons)}
    for edit in edits:
        if edit['name'] == 'Shared Memory':
            cuts[edit['at']] = 'Fleet view to owner-recorded Memory explanation'
    return cuts


def source_time(edit, time):
    if edit['scene'] != 'footage':
        return edit['start']
    return edit['start'] + time * (edit['end'] - edit['start']) / edit['duration']


def camera_at(edit, time):
    if edit['source'] == 'board':
        u = min(1, max(0, (edit['at'] + time - edit.get('groupAt', 8)) / edit.get('groupDuration', 7)))
        progress = u + .45 * math.sin(2 * math.pi * u) / (2 * math.pi)
        return tuple(mix(a, b, progress) for a, b in zip((350, 300, 1680), (850, 310, 1160)))
    u = min(1, max(0, time / edit['duration']))
    if edit['name'] == 'Reports arrive':
        progress = 1 - (1 - u) ** 3
    elif edit['source'] == 'plugins' or edit['name'] == 'Fleet summary':
        progress = ease((u - .3) / .7)
    else:
        progress = ease(u)
    return tuple(mix(a, b, progress) for a, b in zip(edit['camera'], edit['target']))


def camera_velocity(edit, time):
    board = edit['source'] == 'board'
    duration = edit.get('groupDuration', 7) if board else edit['duration']
    u = (edit['at'] + time - edit.get('groupAt', 8)) / duration if board else time / duration
    if board:
        derivative = (1 + .45 * math.cos(2 * math.pi * u)) / duration
    elif edit['name'] == 'Reports arrive':
        derivative = 3 * (1 - u) ** 2 / duration
    elif edit['source'] == 'plugins' or edit['name'] == 'Fleet summary':
        v = max(0, (u - .3) / .7)
        derivative = 30 * v ** 2 * (v - 1) ** 2 / (.7 * duration)
    else:
        derivative = 30 * u ** 2 * (u - 1) ** 2 / duration
    origin, target = ((350, 300, 1680), (850, 310, 1160)) if board else (edit['camera'], edit['target'])
    return tuple((b - a) * derivative for a, b in zip(origin, target))


def crop_at(edit, time):
    x, y, width = camera_at(edit, time)
    box = (x, y, x + width, y + width * 9 / 16)
    require(0 <= box[0] < box[2] <= SOURCE_SIZE[0] and 0 <= box[1] < box[3] <= SOURCE_SIZE[1],
            f'Camera outside source: {edit["name"]} at {time}: {box}')
    return box


def ease(value):
    value = min(1, max(0, value))
    return value * value * value * (value * (value * 6 - 15) + 10)


def mix(a, b, t):
    return a + (b - a) * t


def source_frames(paths, edit):
    count = round(edit['duration'] * FPS)
    footage = edit['scene'] == 'footage'
    first_frame = math.floor(edit['start'] * FPS + 1e-7)
    decode_start = first_frame / FPS
    decode_span = edit['end'] - decode_start
    command = [
        'ffmpeg', '-v', 'error', '-ss', str(decode_start), '-t', str(decode_span), '-i', str(paths[edit['source']]),
        '-vf', 'crop=2048:1396:0:0', '-fps_mode', 'passthrough',
        '-frames:v', str(math.ceil(decode_span * FPS) if footage else 1),
        '-f', 'rawvideo', '-pix_fmt', 'rgb24', '-']
    with bounded_process(command, 120, stdout=subprocess.PIPE) as process:
        decoded = -1
        frame = None
        try:
            for index in range(count):
                desired = math.floor(source_time(edit, index / FPS) * FPS + 1e-7) - first_frame
                while decoded < desired:
                    data = process.stdout.read(SOURCE_SIZE[0] * SOURCE_SIZE[1] * 3)
                    require(len(data) == SOURCE_SIZE[0] * SOURCE_SIZE[1] * 3,
                            f'Incomplete source frame: {edit["name"]}, frame {desired}')
                    frame = Image.frombytes('RGB', SOURCE_SIZE, data)
                    decoded += 1
                yield frame
            while process.stdout.read(SOURCE_SIZE[0] * SOURCE_SIZE[1] * 3):
                pass
            require(process.wait(timeout=10) == 0, 'Source decode failed')
        finally:
            process.stdout.close()


def load_brand_frames(directory, source_path=None):
    directory = Path(directory)
    source_path = Path(source_path or Path(__file__).resolve().parents[1] / 'src/brand/motion.ts')
    manifest_path = directory / 'brand-frames.json'
    require(manifest_path.is_file(), f'Missing authoritative brand frame manifest: {manifest_path}')
    manifest = json.loads(manifest_path.read_text(encoding='utf-8-sig'))
    digest = hashlib.sha256(source_path.read_bytes()).hexdigest()
    require(manifest.get('schemaVersion') == 1 and manifest.get('fps') == FPS and manifest.get('duration') == BRAND_DURATION,
            'Brand frame timing differs from the approved 30fps/1.6s motion')
    require(manifest.get('source', {}).get('sha256') == digest, 'Brand pose source hash is stale')
    poses = manifest.get('poses')
    require(isinstance(poses, list) and len(poses) == OPENING_FRAMES + 1, 'Expected 49 authoritative brand poses')
    require(manifest.get('restFrame') == OPENING_FRAMES and manifest.get('width') == 256 and manifest.get('height') == 256,
            'Brand frame dimensions/rest boundary changed')
    images = []
    for frame, pose in enumerate(poses):
        require(pose.get('frame') == frame and pose.get('time') == frame / FPS, 'Brand pose sequence is incomplete')
        image_path = directory / pose.get('png', '')
        require(image_path.parent == directory.resolve() if image_path.is_absolute() else image_path.parent == directory,
                'Brand frame escaped its export directory')
        require(hashlib.sha256(image_path.read_bytes()).hexdigest() == pose.get('pngSha256'),
                f'Brand frame hash changed: {image_path}')
        with Image.open(image_path) as image:
            require(image.size == (manifest['width'], manifest['height']), 'Unexpected brand SVG raster size')
            converted = image.convert('RGBA')
            require(converted.getpixel((0, 0))[3] == 0, 'Brand frame background must remain transparent')
            images.append(converted.copy())
    rest = poses[-1]
    require(rest['path'] and rest['scaleX'] == rest['scaleY'] == 1 and rest['y'] == 0,
            'Frame 48 is not the authoritative resting mark')
    return dict(images=images, sourceSha256=digest, manifest=manifest)


@lru_cache(maxsize=1)
def wallpaper():
    image = Image.new('RGB', SIZE)
    pixels = image.load()
    for y in range(SIZE[1]):
        for x in range(SIZE[0]):
            blue = math.exp(-(((x - 300) / 650) ** 2 + ((y - 560) / 440) ** 2))
            violet = math.exp(-(((x - 1110) / 400) ** 2 + ((y - 100) / 520) ** 2))
            pixels[x, y] = (int(13 + 10 * blue + 19 * violet), int(17 + 23 * blue + 7 * violet), int(23 + 40 * blue + 34 * violet))
    return image


@lru_cache(maxsize=20)
def face(path, size):
    return ImageFont.truetype(str(path), size)


def text(image, value, xy, font, size=32, color=WHITE, anchor='mm'):
    ImageDraw.Draw(image).text(xy, value, font=face(font, size), fill=color, anchor=anchor)


def window(frame, width, height, font, content_box=None):
    image = Image.new('RGB', (width, height), '#141b24')
    content = frame.crop(content_box or (5, 50, 2045, 1380))
    content.thumbnail((width - 4, height - 35), Image.Resampling.LANCZOS)
    image.paste(content, ((width - content.width) // 2, 33))
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle((0, 0, width-1, height-1), 11, outline='#424e62', width=2)
    for i, color in enumerate(['#ff6058', '#ffbd2e', '#28c840']):
        draw.ellipse((13 + i * 17, 11, 21 + i * 17, 19), fill=color)
    text(image, 'Domios', (width / 2, 16), font, 12, '#aebacf')
    return image


def desktop(frame, time, font, duration=4):
    image = wallpaper().copy()
    elapsed = max(0, time - duration * .0875) * 4 / duration
    frequency = math.sqrt(360 - 14 ** 2)
    opening = 1 - math.exp(-14 * elapsed) * (math.cos(frequency * elapsed) + 14 / frequency * math.sin(frequency * elapsed))
    if elapsed >= 1.2:
        opening = 1
    push = ease((time - duration * .375) / (duration * .625 - .15))
    if push >= 1:
        return frame.resize(SIZE, Image.Resampling.LANCZOS, box=(5, 50, 2045, 1197.5))
    draw = ImageDraw.Draw(image)
    draw.rectangle((0, 0, 1280, 25), fill='#121827')
    text(image, 'Domios', (26, 12), font, 12, anchor='lm')
    text(image, 'Your desktop', (1252, 12), font, 12, '#aebacf', 'rm')
    width = round(mix(670, 1284, push) * mix(.8, 1, opening))
    height = round((width - 4) * 1330 / 2040) + 35
    app = window(frame, width, height, font)
    x, y = (1280-width)//2, round(mix(116, -33, push) + (1-opening)*70)
    if opening > 0:
        image.paste(app, (x,y))
    if time < duration * .7:
        title(image, 'A home for agents', time, duration * .7, font)
    return image


def curve(a, b, c, d, t):
    return tuple((1-t)**3*a[i] + 3*(1-t)**2*t*b[i] + 3*(1-t)*t*t*c[i] + t**3*d[i] for i in range(2))


def network(frame, time, font, duration=4):
    image = wallpaper().copy()
    progress = ease(time / (duration * .7))
    centers = [(mix(640, 283, progress), mix(345, 325, progress)),
               (mix(640, 980, progress), mix(345, 226, progress)),
               (mix(640, 931, progress), mix(345, 542, progress))]
    draw = ImageDraw.Draw(image)
    for index in [1, 2]:
        a = (centers[0][0]+170, centers[0][1])
        d = (centers[index][0]-155, centers[index][1])
        b, c = (a[0]+140, a[1]), (d[0]-140, d[1])
        points = [curve(a,b,c,d,i/100) for i in range(101)]
        draw.line(points, fill='#355c80', width=2)
        for offset in [0, .42]:
            t = (time / duration * 2.2 + offset + index*.2) % 1
            radius = ease(t / .12) * (1 - ease((t - .88) / .12))
            x,y = curve(a,b,c,d,t)
            draw.ellipse((x-7*radius,y-4*radius,x+7*radius,y+4*radius),fill=BLUE)
            x2,y2 = curve(a,b,c,d,max(0,t-.035))
            draw.ellipse((x2-3*radius,y2-3*radius,x2+3*radius,y2+3*radius),fill='#94c7ff')
    sizes = [(round(mix(1120, 440, progress)), round(mix(660, 310, progress))), (342,238), (342,238)]
    for i in [2,1,0]:
        w,h = sizes[i]
        x,y = round(centers[i][0]-w/2), round(centers[i][1]-h/2)
        app = window(frame,w,h,font)
        image.paste(app,(x,y))
        draw = ImageDraw.Draw(image)
        draw.rounded_rectangle((x-5,y-5,x+w+5,y+h+5),12,outline='#667891',width=3)
        if i==0:
            draw.polygon([(x-30,y+h+8),(x+w+30,y+h+8),(x+w+12,y+h+21),(x-12,y+h+21)],fill='#6c7b92')
        else:
            draw.rectangle((x+w//2-13,y+h+6,x+w//2+13,y+h+18),fill='#667891')
            draw.rounded_rectangle((x+w//2-55,y+h+17,x+w//2+55,y+h+23),3,fill='#667891')
        if progress > .8:
            text(image,['YOUR LAPTOP','YOUR DESKTOP','YOUR SERVER'][i],(x+w/2,y+h+43),font,12,'#aebacf')
    title(image,'Every machine. Connected.',time,duration,font)
    return image


def title(image, value, time, duration, font):
    opacity = ease(time/.4) * (1-ease((time-duration+.45)/.45))
    if opacity <= 0:
        return
    layer = Image.new('RGBA', SIZE)
    draw = ImageDraw.Draw(layer)
    y = round(55 + 12 * (1-ease(time/.5)))
    box = draw.textbbox((0,0),value,font=face(font,30))
    width = box[2]-box[0]+52
    draw.rounded_rectangle((640-width/2,y-28,640+width/2,y+29),16,fill=(13,17,23,238))
    draw.text((640,y-1),value,font=face(font,30),fill=WHITE,anchor='mm')
    layer.putalpha(layer.getchannel('A').point(lambda v: round(v*opacity)))
    image.paste(layer,(0,0),layer)


def threshold(size):
    image = Image.new('RGBA',(size,size))
    points=[(8,8),(32,8)]
    points += [curve((32,8),(46,8),(56,18),(56,32),i/60) for i in range(61)]
    points += [curve((56,32),(56,46),(46,56),(32,56),i/60) for i in range(61)]
    points += [(24,56),(40,40),(40,24),(24,24),(24,40),(8,56)]
    ImageDraw.Draw(image).polygon([(x*size/64,y*size/64) for x,y in points],fill=BLUE)
    return image


def endcard(time, frame, font, duration=4):
    image = wallpaper().copy()
    image = Image.blend(image, desktop(frame, 0, font), ease((time - duration + 1) / .8))
    alpha = ease(time/.55) * (1-ease((time-duration+1)/.8))
    if alpha <= 0:
        return image
    layer = Image.new('RGBA',SIZE)
    logo = threshold(256).resize((184,184),Image.Resampling.LANCZOS)
    layer.paste(logo,(640-logo.width//2,240-logo.height//2),logo)
    text(layer,'domios',(640,395),font,68)
    text(layer,'domios.dev',(640,474),font,26,BLUE)
    layer.putalpha(layer.getchannel('A').point(lambda v: round(v*alpha)))
    image.paste(layer,(0,0),layer)
    return image


@lru_cache(maxsize=1)
def ending_mesh():
    require(hashlib.sha256(ENDING_MESH_PATH.read_bytes()).hexdigest() == ENDING_MESH_SHA256,
            'Owner-provided ending screenshot changed')
    with Image.open(ENDING_MESH_PATH) as source:
        require(source.size == (3496, 2079), 'Unexpected ending screenshot dimensions')
        return source.convert('RGB')


def agent_ending(global_frame, frame, font, brand_rest, memory_frame=None):
    image = wallpaper().copy()
    screen = (window(memory_frame, 790, 522, font) if memory_frame is not None else
              window(ending_mesh(), 790, 522, font, (334, 50, 3496, 2079))
              if global_frame >= 1140 else window(frame, 790, 522, font))
    image.paste(screen, (64, 112))
    mark = brand_rest.resize((64, 64), Image.Resampling.LANCZOS)
    image.paste(mark, (930, 210), mark)
    if memory_frame is not None:
        heading = 'One memory.\nEvery agent.'
    elif global_frame < 1140:
        heading = 'Capabilities for\nyour workspace.'
    elif global_frame < MEMORY_END_FRAME:
        heading = 'Agents across\nyour machines.'
    else:
        heading = 'Your next idea.\nA whole team\nbehind you.'
    draw = ImageDraw.Draw(image)
    draw.multiline_text((930, 292), heading, font=face(font, 34 if global_frame >= MEMORY_END_FRAME else 32),
                        fill=WHITE, spacing=4, align='left')
    if memory_frame is not None:
        draw.text((930, 448), 'Shared on this machine.', font=face(font, 14), fill=WHITE)
    draw.text((930, 514), 'domios.dev', font=face(font, 20), fill=BLUE, anchor='lm')
    return image


def opening_mark_opacity(frame):
    if frame < OPENING_FADE_START_FRAME:
        return 1.0
    progress = min(1.0, (frame - OPENING_FADE_START_FRAME) /
                   (OPENING_FRAMES - OPENING_FADE_START_FRAME))
    return 0.5 + 0.5 * math.cos(math.pi * progress)


def render(edit, time, frame, font):
    if edit['scene']=='desktop':
        return desktop(frame,time,font,edit['duration'])
    if edit['scene']=='network':
        return network(frame,time,font,edit['duration'])
    if edit['scene']=='end':
        return endcard(time,frame,font,edit['duration'])
    image = frame.resize(SIZE, Image.Resampling.LANCZOS, box=crop_at(edit, time))
    if edit['title']:
        title(image,edit['title'],time,min(2.6,edit['duration']),font)
    return image


def render_timeline_frame(edit, time, frame, font, brand_frames, memory_frame=None):
    frame_number = round((edit['at'] + time) * FPS)
    if 0 <= frame_number < OPENING_FRAMES:
        image = render(edit, time, frame, font)
        mark = brand_frames['images'][frame_number].resize((164, 164), Image.Resampling.LANCZOS)
        opacity = opening_mark_opacity(frame_number)
        mark.putalpha(mark.getchannel('A').point(lambda value: round(value * opacity)))
        image.paste(mark, (92, 276), mark)
        return image
    if ENDING_START_FRAME <= frame_number < ENDING_END_FRAME:
        return agent_ending(frame_number, frame, font, brand_frames['images'][-1], memory_frame)
    return render(edit, time, frame, font)
