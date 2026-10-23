"""Draw the knob tray/window icons (stdlib only): a filled circle with a pointer, 4x supersampled.
Writes knob_on.ico (accent) and knob_off.ico (grey) as single-frame PNG-in-ICO. Run: python make_icons.py"""
import math, struct, zlib, pathlib

SIZE, SS = 48, 4  # output px, supersampling factor

def png(rgba, n):
    raw = b"".join(b"\0" + bytes(rgba[y * n * 4:(y + 1) * n * 4]) for y in range(n))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0)) + \
           chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")

def seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0, min(1, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))

def draw(body, rim, pointer):
    n, c = SIZE, SIZE / 2
    r_out, r_in = SIZE * 0.47, SIZE * 0.40
    ang = math.radians(-50)                               # pointer towards upper right
    tip = (c + math.cos(ang) * r_in * 0.78, c + math.sin(ang) * r_in * 0.78)
    out = bytearray(n * n * 4)
    for y in range(n):
        for x in range(n):
            acc = [0.0, 0.0, 0.0, 0.0]
            for sy in range(SS):
                for sx in range(SS):
                    px, py = x + (sx + .5) / SS, y + (sy + .5) / SS
                    d = math.hypot(px - c, py - c)
                    if d > r_out: continue
                    col = rim if d > r_in else body
                    if seg_dist(px, py, c, c, *tip) < SIZE * 0.065: col = pointer
                    for i in range(3): acc[i] += col[i]
                    acc[3] += 255
            k = SS * SS
            a = acc[3] / k
            if a:
                for i in range(3): out[(y * n + x) * 4 + i] = round(acc[i] / (acc[3] / 255))
            out[(y * n + x) * 4 + 3] = round(a)
    return png(out, n)

def ico(png_bytes):
    return struct.pack("<HHH", 0, 1, 1) + struct.pack("<BBBBHHII", SIZE, SIZE, 0, 0, 1, 32, len(png_bytes), 22) + png_bytes

here = pathlib.Path(__file__).parent
(here / "knob_on.ico").write_bytes(ico(draw(body=(0, 120, 212), rim=(0, 84, 153), pointer=(255, 255, 255))))
(here / "knob_off.ico").write_bytes(ico(draw(body=(138, 138, 138), rim=(96, 96, 96), pointer=(235, 235, 235))))
print("wrote knob_on.ico, knob_off.ico")
