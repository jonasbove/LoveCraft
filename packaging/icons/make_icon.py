"""Lifts the engraved octopus off its flat coral background and builds rounded-square icons."""
import sys
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

src = np.asarray(Image.open("source-octopus.png").convert("RGB")).astype(np.float64) / 255.0
h, w, _ = src.shape
# Background colour: the median of a border strip.
border = np.concatenate([src[:20].reshape(-1, 3), src[-20:].reshape(-1, 3), src[:, :20].reshape(-1, 3), src[:, -20:].reshape(-1, 3)])
bg = np.median(border, axis=0)
# Each pixel is a grey engraving value v with coverage a over bg: p_c = a*v + (1-a)*bg_c.
# Unknowns u=a*v and a: p_c - bg_c = u - a*bg_c, solved per pixel by least squares over r,g,b.
d = src - bg
A = np.stack([np.ones(3), -bg], axis=1)            # 3x2
pinv = np.linalg.pinv(A)                           # 2x3
sol = d @ pinv.T                                   # h,w,2
u, a = sol[..., 0], np.clip(sol[..., 1], 0, 1)
v = np.where(a > 1e-3, np.clip(u / np.maximum(a, 1e-3), 0, 1), 0)
# Soften the residue of palette quantisation at the edges.
a = np.where(a < 0.06, 0, a)
alpha = Image.fromarray((a * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(0.6))
grey = Image.fromarray((v * 255).astype(np.uint8))
cut = Image.merge("LA", (grey, alpha))
bbox = alpha.point(lambda x: 255 if x > 40 else 0).getbbox()
cut = cut.crop(bbox)
cut.save("octopus-cutout.png")
print("bg", (bg * 255).round(), "bbox", bbox)

def icon(size, top, bottom, tint, scale, name, glow=None):
    S = size * 4                                    # supersample
    # Vertical/radial background: lighter centre, darker edges.
    yy, xx = np.mgrid[0:S, 0:S] / S
    r = np.sqrt((xx - 0.5) ** 2 + (yy - 0.42) ** 2) / 0.75
    t = np.clip(r, 0, 1)[..., None]
    col = np.array(top) * (1 - t) + np.array(bottom) * t
    base = Image.fromarray((col * 255).astype(np.uint8), "RGB").convert("RGBA")
    # The octopus, tinted: black lines stay ink, white highlights take the tint.
    g = np.asarray(cut.getchannel("L")).astype(np.float64) / 255.0
    ink = np.array([0.02, 0.03, 0.03])
    rgb = ink * (1 - g[..., None]) + np.array(tint) * g[..., None]
    oc = Image.fromarray((rgb * 255).astype(np.uint8), "RGB")
    oc.putalpha(cut.getchannel("A"))
    cw, ch = oc.size
    k = (S * scale) / max(cw, ch)
    oc = oc.resize((round(cw * k), round(ch * k)), Image.LANCZOS)
    x = (S - oc.width) // 2
    y = (S - oc.height) // 2
    if glow:
        halo = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        sh = Image.new("RGBA", oc.size, tuple(int(c * 255) for c in glow) + (0,))
        sh.putalpha(oc.getchannel("A").point(lambda p: p * 0.55))
        halo.alpha_composite(sh, (x, y))
        base.alpha_composite(halo.filter(ImageFilter.GaussianBlur(S * 0.02)))
    base.alpha_composite(oc, (x, y))
    # Rounded corners like CraftCloud's (radius about 18% of the side), full bleed.
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, S - 1, S - 1], radius=int(S * 0.18), fill=255)
    base.putalpha(mask)
    base.resize((size, size), Image.LANCZOS).save(name)

variants = {
    "abyss":   dict(top=(0.10, 0.17, 0.15), bottom=(0.02, 0.04, 0.04), tint=(0.86, 0.90, 0.80), glow=(0.30, 0.55, 0.45)),
    "drowned": dict(top=(0.07, 0.11, 0.17), bottom=(0.01, 0.02, 0.05), tint=(0.84, 0.88, 0.90), glow=(0.25, 0.40, 0.60)),
    "ritual":  dict(top=(0.20, 0.06, 0.08), bottom=(0.04, 0.01, 0.02), tint=(0.92, 0.86, 0.78), glow=(0.55, 0.15, 0.15)),
    "ink":     dict(top=(0.11, 0.11, 0.10), bottom=(0.02, 0.02, 0.02), tint=(0.90, 0.86, 0.74), glow=None),
}
for n, v_ in variants.items():
    icon(512, v_["top"], v_["bottom"], v_["tint"], 1.06, f"variant-{n}.png", v_["glow"])
