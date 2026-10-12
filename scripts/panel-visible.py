#!/usr/bin/env python3
"""Is IGRIS's assistant panel painted? Counts pixels of the panel colour
(#161618 in the dark theme, #ffffff in the light one) in the bottom part of
a screenshot, versus the top (where the app underneath shows). Exit 0 when
the bottom has a panel-sized patch of it and the top doesn't."""
import sys

from PIL import Image

img = Image.open(sys.argv[1]).convert("RGB")
w, h = img.size
px = img.load()


def share(colour, y0, y1):
    hits = total = 0
    for y in range(y0, y1, 2):
        for x in range(0, w, 2):
            total += 1
            if all(abs(c - k) <= 6 for c, k in zip(px[x, y], colour)):
                hits += 1
    return hits / max(total, 1)


ok = False
for name, colour in (("dark", (0x16, 0x16, 0x18)), ("light", (0xFF, 0xFF, 0xFF))):
    bottom, top = share(colour, int(h * 0.72), int(h * 0.97)), share(colour, int(h * 0.05), int(h * 0.30))
    print(f"{name} panel colour: bottom {bottom:.0%}, top {top:.0%}")
    ok = ok or (bottom > 0.35 and top < bottom / 2)
sys.exit(0 if ok else 1)
