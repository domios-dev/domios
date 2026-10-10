from pathlib import Path
import argparse
import json
import shutil
import subprocess
import xml.etree.ElementTree as ET

from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from PIL import Image


ROOT = Path(__file__).resolve().parent
PUBLIC = ROOT.parents[1] / "public"
WORK = ROOT.parent / ".render" / "final"
MARK = ET.parse(ROOT.parent / "concepts/03-threshold/color.svg").getroot().find("{http://www.w3.org/2000/svg}path").get("d")
BLUE, DARK, LIGHT, INK = "#58a6ff", "#0d1117", "#f6f8fa", "#1f2328"


def svg(width, height, body, title="Domios"):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="{title}">{body}</svg>\n'


def mark(color="currentColor", transform=""):
    return f'<path fill="{color}" fill-rule="evenodd" d="{MARK}"' + (f' transform="{transform}"' if transform else '') + '/>'


def outline(value, font, size, x, baseline, fill="currentColor", tracking=0):
    glyphs = font.getGlyphSet()
    cmap = font.getBestCmap()
    scale = size / font["head"].unitsPerEm
    pieces = []
    initial = x
    for char in value:
        name = cmap[ord(char)]
        pen = SVGPathPen(glyphs)
        glyphs[name].draw(TransformPen(pen, (scale, 0, 0, -scale, x, baseline)))
        pieces.append(pen.getCommands())
        x += font["hmtx"].metrics[name][0] * scale + tracking
    return f'<path fill="{fill}" d="{" ".join(pieces)}"/>', x - initial - tracking


def write(name, content):
    (ROOT / name).write_text(content, encoding="utf-8")


def generate_vectors(font_dir):
    font = TTFont(font_dir / "segoeuib.ttf")
    regular = TTFont(font_dir / "segoeui.ttf")
    mono = TTFont(font_dir / "consola.ttf")
    for name, color in [("logo.svg", BLUE), ("logo-mono.svg", "currentColor"), ("logo-white.svg", "#ffffff")]:
        write(name, svg(64, 64, mark(color)))
    adaptive = '<style>.mark{fill:#0969da}@media(prefers-color-scheme:dark){.mark{fill:#58a6ff}}</style>'
    write("favicon.svg", svg(64, 64, adaptive + f'<path class="mark" fill-rule="evenodd" d="{MARK}"/>'))
    word, width = outline("domios", font, 52, 84, 55, tracking=-1.7)
    lock_style = '<style>svg{color:#1f2328}.symbol{fill:#0969da}@media(prefers-color-scheme:dark){svg{color:#f0f6fc}.symbol{fill:#58a6ff}}</style>'
    symbol = f'<path class="symbol" fill-rule="evenodd" d="{MARK}" transform="translate(0 4) scale(1.125)"/>'
    write("wordmark-horizontal.svg", svg(round(84 + width + 4), 80, lock_style + symbol + word))
    compact_word, compact_width = outline("domios", font, 43, 0, 167, tracking=-1.3)
    compact_word = f'<g transform="translate({(192 - compact_width) / 2} 0)">{compact_word}</g>'
    symbol = f'<path class="symbol" fill-rule="evenodd" d="{MARK}" transform="translate(40 0) scale(1.75)"/>'
    write("wordmark-compact.svg", svg(192, 184, lock_style + symbol + compact_word))
    og = f'<path fill="{DARK}" d="M0 0H1200V630H0Z"/>'
    og += '<path stroke="#283442" d="M64 112H1136M64 530H1136"/>'
    og += mark(BLUE, "translate(54 35) scale(.85)")
    og += outline("domios", font, 34, 116, 75, "#f0f6fc", -1)[0]
    og += outline("Agents", regular, 100, 64, 261, "#f0f6fc", -4)[0]
    og += outline("live here.", regular, 100, 64, 366, BLUE, -4)[0]
    og += outline("The distributed OS for AI agents", regular, 27, 68, 463, "#c9d1d9", -.2)[0]
    og += outline("domios.dev", mono, 22, 68, 579, "#c9d1d9")[0]
    og += mark(BLUE, "translate(793 154) scale(4.9)")
    write("og-image.svg", svg(1200, 630, og, "Domios. Agents live here. The distributed OS for AI agents. domios.dev"))
    write("site.webmanifest", json.dumps({
        "id": "/", "name": "Domios — Agents live here.", "short_name": "Domios",
        "description": "The distributed OS for AI agents.", "start_url": "/", "scope": "/", "display": "standalone",
        "background_color": DARK, "theme_color": DARK,
        "icons": [
            {"src": "/icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any"},
            {"src": "/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any"},
            {"src": "/maskable-512.png", "sizes": "512x512", "type": "image/png", "purpose": "maskable"},
        ],
    }, ensure_ascii=False, indent=2) + "\n")


def rasterize(browser):
    WORK.mkdir(parents=True, exist_ok=True)
    tiles = [svg(64, 64, mark(BLUE)),
             svg(64, 64, f'<path fill="{DARK}" d="M0 0H64V64H0Z"/>' + mark(BLUE)),
             svg(64, 64, f'<path fill="{DARK}" d="M0 0H64V64H0Z"/>' + mark(BLUE, "translate(10 10) scale(.6875)")),
             svg(64, 64, mark("#0969da"))]
    source = WORK / "raster.html"
    source.write_text('<!doctype html><style>html,body{margin:0;background:transparent}.icons{display:flex}.icons svg{display:block;width:512px;height:512px}.og svg{display:block;width:1200px;height:630px}</style><div class="icons">' + "".join(tiles) + '</div><div class="og">' + (ROOT / "og-image.svg").read_text(encoding="utf-8") + '</div>', encoding="utf-8")
    output = WORK / "raster.png"
    subprocess.run([str(browser), "--headless", "--disable-gpu", "--hide-scrollbars", "--no-first-run",
                    "--default-background-color=00000000", f"--user-data-dir={WORK / 'profile'}",
                    "--window-size=2048,1142", f"--screenshot={output}", source.as_uri()], check=True, capture_output=True, timeout=60)
    atlas = Image.open(output).convert("RGBA")
    dark = atlas.crop((512, 0, 1024, 512))
    for name, size in [("apple-touch-icon.png", 180), ("icon-192.png", 192), ("icon-512.png", 512)]:
        dark.resize((size, size), Image.Resampling.LANCZOS).convert("RGB").save(ROOT / name)
    atlas.crop((1024, 0, 1536, 512)).convert("RGB").save(ROOT / "maskable-512.png")
    atlas.crop((1536, 0, 2048, 512)).save(ROOT / "favicon.ico", format="ICO", sizes=[(16, 16), (32, 32), (48, 48)])
    atlas.crop((0, 512, 1200, 1142)).convert("RGB").save(ROOT / "og-image.png")
    return atlas


def main(browser, font_dir):
    generate_vectors(font_dir)
    rasterize(browser)
    for file in ROOT.iterdir():
        if file.suffix in {".svg", ".png", ".ico", ".webmanifest"}:
            shutil.copyfile(file, PUBLIC / file.name)
    print(f"Generated final assets and copied them to {PUBLIC}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Generate the approved original Threshold brand set.")
    parser.add_argument("--browser", type=Path, default=Path("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"))
    parser.add_argument("--font-dir", type=Path, default=Path("C:/Windows/Fonts"))
    arguments = parser.parse_args()
    main(arguments.browser, arguments.font_dir)
