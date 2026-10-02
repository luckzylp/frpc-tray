#!/usr/bin/env python3
"""Generate icons for frpc-tray: app .ico + tray state .rgba files.

Design: rounded-square tile background + white double-arrow (right over left),
representing bidirectional proxy/forwarding (frp = fast reverse proxy).
"""
import os, sys
from PIL import Image, ImageDraw

ASSETS = os.path.join(os.path.dirname(__file__), "..", "src", "assets")
os.makedirs(ASSETS, exist_ok=True)

BRAND = (13, 148, 136, 255)      # teal #0D9488 (app/exe icon)
IDLE = (156, 163, 175, 255)      # gray  #9CA3AF
RUNNING = (34, 197, 94, 255)     # green #22C55E
STOPPED = (239, 68, 68, 255)     # red   #EF4444

def draw(size, bg):
    """Render the icon design at `size` px on a transparent RGBA canvas."""
    s = size
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # rounded-square tile background
    r = max(2, s // 5)
    d.rounded_rectangle((0, 0, s - 1, s - 1), radius=r, fill=bg)

    # subtle inner highlight border (darker shade of bg) for depth
    def darken(c, f=0.7):
        return (max(0, int(c[0] * f)), max(0, int(c[1] * f)), max(0, int(c[2] * f)), 255)
    bw = max(1, s // 32)
    d.rounded_rectangle((0, 0, s - 1, s - 1), radius=r, outline=darken(bg), width=bw)

    white = (255, 255, 255, 255)

    # arrow geometry helpers
    def arrow(y_mid, direction):
        # shaft
        sh = max(2, s // 10)            # shaft thickness
        x0 = s * 22 // 100
        x1 = s * 58 // 100
        d.rectangle((x0, y_mid - sh // 2, x1, y_mid + sh // 2), fill=white)
        # head triangle
        head = s * 22 // 100
        if direction == "right":
            tip = (s * 78 // 100, y_mid)
            base_top = (x1 - head, y_mid - head)
            base_bot = (x1 - head, y_mid + head)
        else:
            tip = (s * 22 // 100, y_mid)
            base_top = (x0 + head, y_mid - head)
            base_bot = (x0 + head, y_mid + head)
        d.polygon([tip, base_top, base_bot], fill=white)

    # upper arrow -> right, lower arrow <- left
    arrow(s * 38 // 100, "right")
    arrow(s * 62 // 100, "left")

    return img

def save_rgba(img, path):
    raw = img.convert("RGBA").tobytes()
    with open(path, "wb") as f:
        f.write(raw)

def main():
    # app icon: multi-resolution .ico from a 256 master
    master = draw(256, BRAND)
    ico_path = os.path.join(ASSETS, "app.ico")
    sizes = [(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)]
    master.save(ico_path, format="ICO", sizes=sizes)
    print("wrote", ico_path)

    # tray state icons: raw RGBA at 64x64
    for name, color in [("tray_idle", IDLE), ("tray_running", RUNNING), ("tray_stopped", STOPPED)]:
        img = draw(64, color)
        p = os.path.join(ASSETS, f"{name}.rgba")
        save_rgba(img, p)
        print("wrote", p, f"({os.path.getsize(p)} bytes)")

if __name__ == "__main__":
    main()
