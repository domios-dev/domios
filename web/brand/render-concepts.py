from pathlib import Path
import argparse
import json
import subprocess

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parent / "concepts"
INK = "#0d1117"
PAPER = "#f6f8fa"
BLUE = "#58a6ff"
DEEP_BLUE = "#0969da"
MUTED = "#828a92"
ROUND = 1
CONCEPTS = [
    {
        "slug": "01-hearth",
        "name": "Hearth",
        "idea": "A house with a softer side.",
        "rationale": [
            "A pitched roof flows into a rounded D shoulder. The broad open doorway",
            "makes a welcoming home; the asymmetric silhouette gives it character.",
        ],
        "path": "M8 24 24 8H32C46 8 56 18 56 32V56H40V32H24V56H8Z",
        "note": "One contour / 16-unit doorway / roof meets a D shoulder",
    },
    {
        "slug": "02-resident",
        "name": "Resident",
        "idea": "A little presence in the machine.",
        "rationale": [
            "A rounded presence with a single lit window and a jaunty folded hem.",
            "Part spirit, part tiny dwelling: playful without a robot face or fine details.",
        ],
        "path": "M8 56V28C8 16 18 8 32 8S56 16 56 28V56L40 48 24 56V48ZM28 24V36H40V24Z",
        "note": "One compound path / 12-unit window / bold folded hem",
    },
    {
        "slug": "03-threshold",
        "name": "Threshold",
        "idea": "The D is an open door.",
        "rationale": [
            "A sturdy D opens at its lower corner, turning its counter into an entrance.",
            "The diagonal cut suggests stepping inside: a compact symbol for an agent OS.",
        ],
        "path": "M8 8H32C46 8 56 18 56 32S46 56 32 56H24L40 40V24H24V40L8 56Z",
        "note": "One contour / 16-unit interior / diagonal entry cut",
    },
    {
        "slug": "04-commons",
        "name": "Commons",
        "idea": "Many agents. One place to belong.",
        "rationale": [
            "Two rooftops share a single courtyard, turning a house into a small community.",
            "The broad, twin-peaked silhouette suggests agents living and working together.",
        ],
        "path": "M4 24 20 8 32 20 44 8 60 24V56H4ZM24 32V48H40V32Z",
        "note": "One compound path / 16-unit courtyard / two roofs, one home",
    },
]


def svg(concept, color, background=None):
    back = f'<path fill="{background}" d="M0 0H64V64H0Z"/>' if background else ""
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" '
        'role="img" aria-labelledby="title desc">\n'
        f'  <title id="title">Domios: {concept["name"]}</title>\n'
        f'  <desc id="desc">{concept["idea"]}</desc>\n'
        f'  {back}<path fill="{color}" fill-rule="evenodd" d="{concept["path"]}"/>\n'
        '</svg>\n'
    )


def render_atlas(browser):
    work = Path(__file__).resolve().parent / ".render" / f"round-{ROUND}"
    work.mkdir(parents=True, exist_ok=True)
    blocks = []
    for concept in CONCEPTS:
        for color in [BLUE, DEEP_BLUE, "#000000", "#ffffff"]:
            blocks.append(svg(concept, color))
    html = (
        '<!doctype html><html><head><style>'
        'html,body{margin:0;background:transparent}body{display:grid;grid-template-columns:repeat(4,512px)}'
        'svg{display:block;width:512px;height:512px}</style></head><body>'
        + "".join(blocks) + '</body></html>'
    )
    source = work / "atlas.html"
    output = work / "atlas.png"
    source.write_text(html, encoding="utf-8")
    command = [
        str(browser), "--headless", "--disable-gpu", "--hide-scrollbars",
        "--no-first-run", "--no-default-browser-check", "--default-background-color=00000000",
        f"--user-data-dir={work / 'profile'}", "--window-size=2048,2048",
        f"--screenshot={output}", source.as_uri(),
    ]
    subprocess.run(command, check=True, capture_output=True, timeout=60)
    atlas = Image.open(output).convert("RGBA")
    if atlas.size != (2048, 2048):
        raise ValueError(f"Unexpected raster dimensions: {atlas.size}")
    return atlas


def font(size, weight="regular"):
    names = {"regular": "segoeui.ttf", "bold": "segoeuib.ttf", "mono": "consola.ttf"}
    return ImageFont.truetype(str(FONT_DIR / names[weight]), size)


def text(draw, xy, value, size=20, fill=INK, weight="regular"):
    draw.text(xy, value, font=font(size, weight), fill=fill)


def place(canvas, mark, xy, size):
    scaled = mark if size == 512 else mark.resize((size, size), Image.Resampling.LANCZOS)
    canvas.alpha_composite(scaled, xy)


def lockup(canvas, draw, mark, xy, light=False):
    x, y = xy
    place(canvas, mark, (x, y), 64)
    text(draw, (x + 78, y - 4), "domios", 52, PAPER if light else INK, "bold")
    text(draw, (x + 80, y + 62), "Agents live here.", 20, MUTED if light else "#57606a")


def browser(canvas, draw, mark, xy, dark=False):
    x, y = xy
    chrome, active, fg = ("#222a35", INK, PAPER) if dark else ("#dfe5ed", "#ffffff", INK)
    draw.rounded_rectangle((x, y, x + 570, y + 112), 12, fill=chrome)
    draw.rounded_rectangle((x + 14, y + 12, x + 337, y + 51), 8, fill=active)
    place(canvas, mark, (x + 28, y + 24), 16)
    text(draw, (x + 54, y + 20), "Domios — Agents live here.", 16, fg)
    text(draw, (x + 311, y + 18), "×", 18, fg)
    text(draw, (x + 357, y + 17), "+", 22, fg)
    draw.rounded_rectangle((x + 16, y + 64, x + 554, y + 98), 7, fill=active)
    text(draw, (x + 34, y + 69), "domios.dev", 16, fg)


def sheet(concept, marks):
    canvas = Image.new("RGBA", (1600, 1420), PAPER)
    draw = ImageDraw.Draw(canvas)
    draw.rectangle((0, 0, 1599, 167), fill=INK)
    heading = "DOMIOS / IDENTITY EXPLORATIONS" + (f" / ROUND {ROUND}" if ROUND > 1 else "")
    text(draw, (64, 29), heading, 18, BLUE, "mono")
    title = concept["name"] if ROUND == 3 else f'{concept["slug"][:2]}   {concept["name"]}'
    text(draw, (64, 68), title, 50, PAPER, "bold")
    if ROUND > 1:
        text(draw, (970, 99), "THE DISTRIBUTED OS FOR AI AGENTS", 18, MUTED, "mono")
    else:
        text(draw, (1000, 99), "THE OS FOR AI AGENTS", 18, MUTED, "mono")
    text(draw, (64, 194), concept["idea"], 30, INK, "bold")
    for line, value in enumerate(concept["rationale"]):
        text(draw, (64, 244 + line * 28), value, 20, "#57606a")
    draw.rounded_rectangle((64, 330, 640, 938), 20, fill="#ffffff")
    text(draw, (94, 351), "512 PX / LIGHT", 16, "#57606a", "mono")
    place(canvas, marks["light"], (96, 399), 512)
    draw.rounded_rectangle((672, 330, 1536, 938), 20, fill=INK)
    text(draw, (714, 357), "WORDMARK / DARK", 16, MUTED, "mono")
    lockup(canvas, draw, marks["dark"], (714, 416), light=True)
    text(draw, (714, 568), "ACTUAL PIXELS / UNALTERED DOWNSCALES", 16, MUTED, "mono")
    for x, size in [(726, 64), (890, 32), (1030, 16)]:
        place(canvas, marks["dark"], (x, 625 + (64 - size) // 2), size)
        text(draw, (x, 711), str(size) + " px", 18, PAPER, "mono")
    text(draw, (714, 792), "ONE INK / SAME GEOMETRY", 16, MUTED, "mono")
    place(canvas, marks["white"], (714, 836), 64)
    draw.rounded_rectangle((814, 824, 902, 912), 10, fill=PAPER)
    place(canvas, marks["black"], (826, 836), 64)
    text(draw, (950, 855), "No strokes. No gradients.", 20, PAPER)
    text(draw, (64, 974), "WORDMARK / LIGHT", 16, "#57606a", "mono")
    lockup(canvas, draw, marks["light"], (64, 1015))
    text(draw, (64, 1156), "ACTUAL PIXELS / LIGHT", 16, "#57606a", "mono")
    for x, size in [(76, 64), (234, 32), (370, 16)]:
        place(canvas, marks["light"], (x, 1200 + (64 - size) // 2), size)
        text(draw, (x, 1273), str(size) + " px", 18, "#57606a", "mono")
    text(draw, (800, 974), "BROWSER / 16 PX FAVICON", 16, "#57606a", "mono")
    browser(canvas, draw, marks["light"], (800, 1017))
    browser(canvas, draw, marks["dark"], (800, 1153), dark=True)
    draw.line((64, 1337, 1536, 1337), fill="#d4dbe3", width=1)
    text(draw, (64, 1360), concept["note"], 17, "#57606a", "mono")
    text(draw, (1190, 1360), "CONCEPT / NOT SELECTED", 16, "#57606a", "mono")
    canvas.convert("RGB").save(ROOT / f'{concept["slug"]}-sheet.png')


def overview(all_marks):
    canvas = Image.new("RGBA", (1600, 1200), INK)
    draw = ImageDraw.Draw(canvas)
    text(draw, (64, 36), "domios", 48, PAPER, "bold")
    subtitle = "Four places for agents to call home." if ROUND == 1 else "Many machines. One home for your agents."
    text(draw, (64, 100), subtitle, 24, MUTED)
    label = "IDENTITY / 01–04" if ROUND == 1 else "IDENTITY / ROUND 2"
    text(draw, (1230, 72), label, 18, BLUE, "mono")
    for i, (concept, marks) in enumerate(zip(CONCEPTS, all_marks)):
        x, y = 64 + (i % 2) * 752, 180 + (i // 2) * 490
        draw.rounded_rectangle((x, y, x + 720, y + 458), 18, fill="#141b24")
        text(draw, (x + 32, y + 22), f'{concept["slug"][:2]} / {concept["name"]}', 23, PAPER, "bold")
        place(canvas, marks["dark"], (x + 220, y + 70), 256)
        text(draw, (x + 32, y + 339), concept["idea"], 21, PAPER)
        text(draw, (x + 32, y + 404), "64", 15, MUTED, "mono")
        place(canvas, marks["dark"], (x + 66, y + 376), 64)
        text(draw, (x + 158, y + 404), "32", 15, MUTED, "mono")
        place(canvas, marks["dark"], (x + 194, y + 392), 32)
        text(draw, (x + 258, y + 404), "16", 15, MUTED, "mono")
        place(canvas, marks["dark"], (x + 294, y + 400), 16)
        text(draw, (x + 510, y + 402), "Agents live here.", 17, MUTED)
    text(draw, (64, 1164), "Same 64-unit geometry at every size. Open individual sheets at 100% to judge favicon pixels.", 17, MUTED)
    canvas.convert("RGB").save(ROOT / "overview.png")


def main(browser):
    ROOT.mkdir(parents=True, exist_ok=True)
    atlas = render_atlas(browser)
    all_marks = []
    for row, concept in enumerate(CONCEPTS):
        folder = ROOT / concept["slug"]
        folder.mkdir(exist_ok=True)
        variants = {
            "color": (BLUE, None), "color-light": (DEEP_BLUE, None),
            "mono": ("currentColor", None), "mono-white": ("#ffffff", None),
            "on-dark": (BLUE, INK), "on-light": (DEEP_BLUE, PAPER),
        }
        for name, (color, background) in variants.items():
            (folder / f"{name}.svg").write_text(svg(concept, color, background), encoding="utf-8")
        marks = {
            key: atlas.crop((column * 512, row * 512, (column + 1) * 512, (row + 1) * 512))
            for column, key in enumerate(["dark", "light", "black", "white"])
        }
        for theme in ["dark", "light"]:
            for size in [512, 64, 32, 16]:
                marks[theme].resize((size, size), Image.Resampling.LANCZOS).save(folder / f"{theme}-{size}.png")
        sheet(concept, marks)
        all_marks.append(marks)
    overview(all_marks)
    print(f"Rendered four concept sheets, overview, 24 SVGs and 32 scale samples to {ROOT}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Render Domios SVG concepts and presentation sheets.")
    parser.add_argument("--font-dir", type=Path, default=Path("C:/Windows/Fonts"))
    parser.add_argument("--browser", type=Path, default=Path("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"))
    parser.add_argument("--round", type=int, choices=[1, 2], default=1)
    arguments = parser.parse_args()
    ROUND = arguments.round
    if ROUND == 2:
        ROOT = ROOT / "round-2"
        CONCEPTS = json.loads((ROOT / "concepts.json").read_text(encoding="utf-8"))
    FONT_DIR = arguments.font_dir
    main(arguments.browser)
