"""Assemble les rendus Blender : forge-bar.ico (16 à 256 px), favicon PNG et logo animé WebP de l'éditeur.

    python assets/icon/build_icon.py     (après render_icon.py)
"""
import glob
import os

from PIL import Image

here = os.path.dirname(os.path.abspath(__file__))
render = os.path.join(here, "render")
ui = os.path.join(here, "..", "..", "crates", "forge-settings", "ui")


def square(im, box):
    """Recadre sur `box` puis centre dans un carré transparent avec une petite marge."""
    c = im.crop(box)
    side = max(c.size)
    out = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    out.paste(c, ((side - c.width) // 2, (side - c.height) // 2))
    return out


icon = Image.open(os.path.join(render, "icon_512.png")).convert("RGBA")
box = icon.getbbox()
big = square(icon, box).resize((256, 256), Image.LANCZOS)
big.save(os.path.join(here, "forge-bar.ico"), sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
big.resize((64, 64), Image.LANCZOS).save(os.path.join(ui, "favicon.png"))

frames = [Image.open(p).convert("RGBA") for p in sorted(glob.glob(os.path.join(render, "frames", "*.png")))]
# Même cadre pour toutes les images, sinon le logo « saute » d'une image à l'autre.
x0 = min(f.getbbox()[0] for f in frames)
y0 = min(f.getbbox()[1] for f in frames)
x1 = max(f.getbbox()[2] for f in frames)
y1 = max(f.getbbox()[3] for f in frames)
small = [square(f, (x0, y0, x1, y1)).resize((112, 112), Image.LANCZOS) for f in frames]
small[0].save(os.path.join(ui, "logo.webp"), save_all=True, append_images=small[1:], duration=70, loop=0, quality=80)
print("ico", os.path.getsize(os.path.join(here, "forge-bar.ico")), "webp", os.path.getsize(os.path.join(ui, "logo.webp")))
