"""Draw the Work Desk app icon and pack it as WorkDesk.icns.

Needs Pillow and macOS's iconutil. Only rerun it to change the icon; the .icns is checked in.
Usage: python3 make_icon.py   (writes WorkDesk.icns next to this file)
"""
import os
import subprocess
import tempfile

from PIL import Image, ImageDraw, ImageFilter

S = 1024
ACCENT_TOP, ACCENT_BOTTOM = (98, 86, 236), (60, 48, 196)
NOTE, NOTE_FOLD, NOTE_LINE = (255, 233, 138), (232, 205, 96), (176, 156, 74)


def rounded(size, box, r, fill):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle(box, r, fill=255)
    layer = Image.new("RGBA", size, fill)
    layer.putalpha(m)
    return layer


def draw():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    tile = (100, 100, 924, 924)

    # drop shadow under the tile, as on other Mac icons
    shadow = rounded((S, S), (100, 116, 924, 940), 185, (20, 16, 60, 110)).filter(ImageFilter.GaussianBlur(18))
    img.alpha_composite(shadow)

    # indigo tile with a vertical gradient
    grad = Image.new("RGBA", (S, S))
    gd = ImageDraw.Draw(grad)
    for y in range(S):
        t = min(max((y - tile[1]) / (tile[3] - tile[1]), 0), 1)
        gd.line([(0, y), (S, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(ACCENT_TOP, ACCENT_BOTTOM)) + (255,))
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle(tile, 185, fill=255)
    img.paste(grad, (0, 0), mask)

    # three kanban lanes of cards
    d = ImageDraw.Draw(img)
    lanes = [(196, [150, 96, 128]), (418, [112, 150]), (640, [96])]
    for x, cards in lanes:
        y = 214
        for h in cards:
            d.rounded_rectangle((x, y, x + 188, y + h), 26, fill=(255, 255, 255, 235))
            d.rounded_rectangle((x + 26, y + 28, x + 130, y + 44), 8, fill=(140, 130, 235, 255))
            y += h + 26

    # sticky note, tilted, with a folded corner
    note = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    nd = ImageDraw.Draw(note)
    nx, ny, n = 560, 520, 300
    nd.rectangle((nx, ny, nx + n, ny + n), fill=NOTE + (255,))
    nd.polygon([(nx + n - 70, ny + n), (nx + n, ny + n - 70), (nx + n, ny + n)], fill=(0, 0, 0, 0))
    nd.polygon([(nx + n - 70, ny + n), (nx + n, ny + n - 70), (nx + n - 70, ny + n - 70)], fill=NOTE_FOLD + (255,))
    for i, w in enumerate([210, 180, 120]):
        nd.rounded_rectangle((nx + 40, ny + 58 + i * 52, nx + 40 + w, ny + 76 + i * 52), 9, fill=NOTE_LINE + (255,))
    note = note.rotate(-7, resample=Image.BICUBIC, center=(nx + n / 2, ny + n / 2))
    note_shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    note_shadow.putalpha(note.getchannel("A").point(lambda a: a * 0.35))
    note_shadow = note_shadow.filter(ImageFilter.GaussianBlur(14))
    img.alpha_composite(note_shadow, (6, 14))
    img.alpha_composite(note)

    # keep everything inside the tile
    clip = Image.new("L", (S, S), 0)
    ImageDraw.Draw(clip).rounded_rectangle(tile, 185, fill=255)
    out = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    out.alpha_composite(shadow)
    out.paste(img, (0, 0), clip)
    return out


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    big = draw()
    with tempfile.TemporaryDirectory() as tmp:
        iconset = os.path.join(tmp, "WorkDesk.iconset")
        os.mkdir(iconset)
        for pt in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                px = pt * scale
                name = f"icon_{pt}x{pt}{'@2x' if scale == 2 else ''}.png"
                big.resize((px, px), Image.LANCZOS).save(os.path.join(iconset, name))
        subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(here, "WorkDesk.icns")], check=True)
    big.save(os.path.join(tempfile.gettempdir(), "workdesk-icon-preview.png"))
    print("Wrote", os.path.join(here, "WorkDesk.icns"))


if __name__ == "__main__":
    main()
