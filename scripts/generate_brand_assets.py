"""Generate web branding from the repository SVG. Requires Pillow.

The source SVG embeds three PNG layers. Composite those layers at their
specified SVG positions without changing the original artwork.
"""
import base64
import io
import shutil
from pathlib import Path
import xml.etree.ElementTree as ET

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'Data/Assets/Grimoire_Icon.svg'
OUTPUT = ROOT / 'web/public'
BACKGROUND = '#b6a3e5'


def generate():
    root = ET.parse(SOURCE).getroot()
    x, y, width, height = map(float, root.attrib['viewBox'].split())
    if x or y:
        raise ValueError('Expected the SVG viewBox to start at the origin.')
    canvas = Image.new('RGBA', (round(width), round(height)))
    for layer in root:
        tag = layer.tag.rsplit('}', 1)[-1]
        if tag in ('title', 'defs'):
            continue
        if tag != 'image' or not layer.attrib.get('href', '').startswith('data:image/png;base64,'):
            raise ValueError('The SVG format changed; review the converter before regenerating.')
        image = Image.open(io.BytesIO(base64.b64decode(layer.attrib['href'].split(',', 1)[1]))).convert('RGBA')
        image = image.resize((round(float(layer.attrib['width'])), round(float(layer.attrib['height']))), Image.Resampling.LANCZOS)
        canvas.alpha_composite(image, (round(float(layer.attrib['x'])), round(float(layer.attrib['y']))))
    size = max(canvas.size)
    icon = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    icon.alpha_composite(canvas, ((size - canvas.width) // 2, (size - canvas.height) // 2))
    OUTPUT.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(SOURCE, OUTPUT / 'logo.svg')
    for filename, dimension in [('favicon-32.png', 32), ('apple-touch-icon.png', 180)]:
        output = icon
        if filename == 'apple-touch-icon.png':
            output = Image.new('RGBA', icon.size, BACKGROUND)
            output.alpha_composite(icon)
        output.resize((dimension, dimension), Image.Resampling.LANCZOS).save(OUTPUT / filename, optimize=True)
    icon.save(OUTPUT / 'favicon.ico', sizes=[(16, 16), (32, 32), (48, 48)])


if __name__ == '__main__':
    generate()
