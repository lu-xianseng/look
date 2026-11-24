"""Generate ScreenKit app icons with no third-party dependencies.

Outputs:
    src-tauri/icons/32x32.png
    src-tauri/icons/128x128.png
    src-tauri/icons/128x128@2x.png
    src-tauri/icons/icon.png
    src-tauri/icons/icon.ico   (32 + 48, BMP entries)
"""

import math
import os
import struct
import zlib

OUT_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "src-tauri", "icons")

BG = (26, 30, 40)
FG = (245, 246, 248)
DOT = (235, 66, 66)

INSET = 0.13
LEN = 0.185
T = 0.042


def _bar(u, v, x0, x1, y0, y1):
    return x0 <= u <= x1 and y0 <= v <= y1


def _brackets(u, v):
    l, r, i, t = LEN, 1.0 - LEN, INSET, T
    if _bar(u, v, i, i + l, i, i + t) or _bar(u, v, i, i + t, i, i + l):
        return True
    if _bar(u, v, r - l, 1.0 - i, i, i + t) or _bar(u, v, 1.0 - i - t, 1.0 - i, i, i + l):
        return True
    if _bar(u, v, i, i + l, 1.0 - i - t, 1.0 - i) or _bar(u, v, i, i + t, r - l, 1.0 - i):
        return True
    if _bar(u, v, r - l, 1.0 - i, 1.0 - i - t, 1.0 - i) or _bar(u, v, 1.0 - i - t, 1.0 - i, r - l, 1.0 - i):
        return True
    return False


def _inside_round_rect(u, v, radius=0.17):
    if u < 0 or v < 0 or u > 1 or v > 1:
        return False
    cx = min(max(u, radius), 1.0 - radius)
    cy = min(max(v, radius), 1.0 - radius)
    return math.hypot(u - cx, v - cy) <= radius


def sample(u, v):
    if not _inside_round_rect(u, v):
        return (0, 0, 0, 0)
    if _brackets(u, v):
        return FG + (255,)
    if math.hypot(u - 0.5, v - 0.5) < 0.098:
        return DOT + (255,)
    return BG + (255,)


def render(size, ss=4):
    px = bytearray(size * size * 4)
    inv = 1.0 / (size * ss)
    for y in range(size):
        for x in range(size):
            r = g = b = a = 0.0
            for sy in range(ss):
                for sx in range(ss):
                    u = (x * ss + sx + 0.5) * inv
                    v = (y * ss + sy + 0.5) * inv
                    cr, cg, cb, ca = sample(u, v)
                    r += cr * ca
                    g += cg * ca
                    b += cb * ca
                    a += ca
            n = ss * ss
            i = (y * size + x) * 4
            if a <= 0.0:
                px[i:i + 4] = b"\x00\x00\x00\x00"
            else:
                px[i] = int(round(r / a))
                px[i + 1] = int(round(g / a))
                px[i + 2] = int(round(b / a))
                px[i + 3] = int(round(a / n))
    return bytes(px)


def png_bytes(size, rgba):
    raw = bytearray()
    for y in range(size):
        raw += b"\x00" + rgba[y * size * 4:(y + 1) * size * 4]

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
            + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))


def ico_entry(size, rgba):
    """32bpp BMP entry inside ICO (BI_RGB, bottom-up, BGRA + AND mask)."""
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, size * size * 4, 0, 0, 0, 0)
    xor = bytearray()
    for y in range(size - 1, -1, -1):
        row = rgba[y * size * 4:(y + 1) * size * 4]
        for x in range(size):
            b, g, r, a = row[x * 4:x * 4 + 4]
            xor += struct.pack("<BBBB", b, g, r, a)
    mask_row_bytes = ((size + 31) // 32) * 4
    andmask = bytearray()
    for y in range(size - 1, -1, -1):
        row = bytearray(mask_row_bytes)
        for x in range(size):
            if rgba[(y * size + x) * 4 + 3] == 0:
                row[x // 8] |= 0x80 >> (x % 8)
        andmask += row
    return header + bytes(xor) + bytes(andmask)


def ico_bytes(images):
    count = len(images)
    out = bytearray(struct.pack("<HHH", 0, 1, count))
    offset = 6 + 16 * count
    dir_entries = b""
    data = b""
    for size, blob in images:
        dir_entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return bytes(out) + dir_entries + data


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    cache = {}
    for size in (32, 48, 128, 256, 512):
        cache[size] = render(size)

    for name, size in (("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)):
        with open(os.path.join(OUT_DIR, name), "wb") as f:
            f.write(png_bytes(size, cache[size]))

    with open(os.path.join(OUT_DIR, "icon.ico"), "wb") as f:
        f.write(ico_bytes([(32, ico_entry(32, cache[32])), (48, ico_entry(48, cache[48]))]))

    print("icons written to", OUT_DIR)


if __name__ == "__main__":
    main()
