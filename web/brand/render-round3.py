from pathlib import Path
import argparse
import importlib.util
import json
import subprocess
import textwrap
import xml.etree.ElementTree as ET

from PIL import Image, ImageDraw


BASE = Path(__file__).resolve().parent
ROOT = BASE / "concepts" / "round-3"
WORK = BASE / ".render" / "round-3"
spec = importlib.util.spec_from_file_location("presentation", BASE / "render-concepts.py")
presentation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(presentation)
presentation.ROOT = ROOT
presentation.ROUND = 3


def concepts():
    original = ET.parse(BASE / "concepts/03-threshold/color.svg")
    path = original.getroot().find("{http://www.w3.org/2000/svg}path").get("d")
    baseline = dict(id="03", name="Original", desc="The selected Threshold direction, unchanged: a D with an open lower corner.", d=path, rule="evenodd")
    return [baseline] + json.loads((ROOT / "logos-round3.json").read_text(encoding="utf-8"))


def adapt(item):
    return dict(slug=f'{item["id"]}-{item["name"].lower()}', name=f'{item["id"]} / {item["name"]}',
                idea=item["name"] + " — Threshold refinement" if item["id"] != "03" else "Original 03 — comparison baseline",
                rationale=textwrap.wrap(item["desc"], 88), path=item["d"],
                note="One connected shape / same geometry at every size")


def render(items, browser):
    WORK.mkdir(parents=True, exist_ok=True)
    colors = [presentation.BLUE, presentation.DEEP_BLUE, "#000000", "#ffffff"]
    blocks = "".join(presentation.svg(adapt(item), color) for item in items for color in colors)
    source = WORK / "atlas.html"
    source.write_text('<!doctype html><style>html,body{margin:0;background:transparent}body{display:grid;grid-template-columns:repeat(4,512px)}svg{width:512px;height:512px}</style>' + blocks, encoding="utf-8")
    output = WORK / "atlas.png"
    subprocess.run([str(browser), "--headless", "--disable-gpu", "--hide-scrollbars", "--no-first-run",
                    "--default-background-color=00000000", f"--user-data-dir={WORK / 'profile'}",
                    f"--window-size=2048,{512 * len(items)}", f"--screenshot={output}", source.as_uri()],
                   capture_output=True, check=True, timeout=60)
    atlas = Image.open(output).convert("RGBA")
    if atlas.size != (2048, 512 * len(items)):
        raise ValueError(f"Unexpected atlas dimensions: {atlas.size}")
    return atlas


def overview(items, marks):
    canvas = Image.new("RGBA", (1600, 2150), presentation.INK)
    draw = ImageDraw.Draw(canvas)
    presentation.text(draw, (64, 28), "domios / Threshold", 44, presentation.PAPER, "bold")
    presentation.text(draw, (64, 93), "Original 03 + six refinements. One open door, seven ways to feel it.", 23, presentation.MUTED)
    summaries = [
        "The selected Threshold direction, unchanged. The comparison baseline.",
        "A 20-unit chamber and wider diagonal entry. More light through the same bowl.",
        "A curved liquid neck. The doorway feels pulled open while the bowl stays familiar.",
        "A broad resident peeks from the jamb. One connected shape, without a tiny separate eye.",
        "A fuller bowl and rounded corners. The same doorway, with a friendlier shoulder.",
        "A broad inner plane suggests a door swung outward. Built into the silhouette.",
        "A larger footprint and heavier walls. The room stays four pixels wide at 16 px.",
    ]
    for i, (item, mark) in enumerate(zip(items, marks)):
        x, y = 64 + i % 2 * 752, 160 + i // 2 * 480
        draw.rounded_rectangle((x, y, x + 720, y + 448), 18, fill="#141b24",
                               outline=presentation.BLUE if i == 0 else None, width=2)
        presentation.text(draw, (x + 28, y + 20), f'{item["id"]} / {item["name"]}', 24, presentation.PAPER, "bold")
        presentation.place(canvas, mark, (x + 230, y + 65), 256)
        for j, line in enumerate(textwrap.wrap(summaries[i], 70)):
            presentation.text(draw, (x + 28, y + 326 + j * 23), line, 17, presentation.PAPER)
        for dx, size in [(28, 64), (170, 32), (290, 16)]:
            presentation.place(canvas, mark, (x + dx, y + 375 + (64 - size) // 2), size)
            presentation.text(draw, (x + dx + size + 14, y + 401), str(size), 16, presentation.MUTED, "mono")
    x, y = 816, 1600
    presentation.text(draw, (x + 30, y + 32), "WHAT TO COMPARE", 20, presentation.BLUE, "mono")
    for j, line in enumerate([
        "03a  Wider chamber / 45-degree cut",
        "03b  Liquid neck / softer entry",
        "03c  Attached resident / occupied room",
        "03d  Fuller bowl / rounded corners",
        "03e  Door plane / open swing",
        "03f  More ink / stronger favicon",
        "",
        "Individual sheets include 512 px masters,",
        "both themes and actual 16 px browser tabs.",
    ]):
        presentation.text(draw, (x + 30, y + 94 + j * 29), line, 19, presentation.PAPER)
    presentation.text(draw, (64, 2094), "Same geometry at 512 / 64 / 32 / 16 px. All concepts remain proposals. No site integration yet.", 18, presentation.MUTED)
    canvas.convert("RGB").save(ROOT / "overview.png")


def main(browser, font_dir):
    presentation.FONT_DIR = font_dir
    items = concepts()
    atlas = render(items, browser)
    all_marks = []
    for row, item in enumerate(items):
        concept = adapt(item)
        folder = ROOT / concept["slug"]
        folder.mkdir(exist_ok=True)
        variants = {"color": (presentation.BLUE, None), "color-light": (presentation.DEEP_BLUE, None),
                    "mono": ("currentColor", None), "mono-white": ("#ffffff", None),
                    "on-dark": (presentation.BLUE, presentation.INK), "on-light": (presentation.DEEP_BLUE, presentation.PAPER)}
        for name, (color, background) in variants.items():
            (folder / f"{name}.svg").write_text(presentation.svg(concept, color, background), encoding="utf-8")
        marks = {name: atlas.crop((col * 512, row * 512, (col + 1) * 512, (row + 1) * 512))
                 for col, name in enumerate(["dark", "light", "black", "white"])}
        for theme in ["dark", "light"]:
            for size in [512, 64, 32, 16]:
                marks[theme].resize((size, size), Image.Resampling.LANCZOS).save(folder / f"{theme}-{size}.png")
        presentation.sheet(concept, marks)
        all_marks.append(marks["dark"])
    overview(items, all_marks)
    print(f"Rendered original + six refinements to {ROOT}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Render the original Threshold beside six refinements.")
    parser.add_argument("--browser", type=Path, default=Path("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"))
    parser.add_argument("--font-dir", type=Path, default=Path("C:/Windows/Fonts"))
    arguments = parser.parse_args()
    main(arguments.browser, arguments.font_dir)
