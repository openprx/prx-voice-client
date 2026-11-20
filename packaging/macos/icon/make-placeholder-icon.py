#!/usr/bin/env python3
"""Draw the PLACEHOLDER application icon as AppIcon-1024.png (standard library only).

A rounded square in the theme's primary colour (#222224, crates/desktop-ui theme)
with a white hexagon outline, the same shape the tray icon draws. Original,
generated from the formulas below; no third-party artwork. Replace the PNG with
real artwork when it exists; the packaging script only needs a 1024x1024 PNG.

Usage: python3 make-placeholder-icon.py [output.png]
"""
import math
import struct
import sys
import zlib

SIZE = 1024
BODY = 824            # macOS icon grid: the shape fills 824 of 1024 px
RADIUS = 185          # corner radius of the rounded square
HEX_R = 250           # hexagon circumradius
STROKE = 56           # hexagon outline width
FILL = (0x22, 0x22, 0x24)
GLYPH = (0xFF, 0xFF, 0xFF)


def sd_round_rect(px, py, half, radius):
    qx = abs(px) - (half - radius)
    qy = abs(py) - (half - radius)
    return math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - radius


def sd_hexagon(px, py, r):
    """Signed distance to a pointy-top regular hexagon with circumradius r."""
    kx, ky, kz = -0.8660254, 0.5, 0.5773503
    apothem = r * 0.8660254
    x, y = abs(px), abs(py)
    d = min(kx * x + ky * y, 0.0)  # reflect into the fundamental wedge
    x -= 2.0 * d * kx
    y -= 2.0 * d * ky
    x -= max(-kz * apothem, min(kz * apothem, x))
    y -= apothem
    return math.hypot(x, y) * (1.0 if y > 0 else -1.0)


def coverage(dist):
    """Anti-aliased coverage from a signed distance (negative = inside)."""
    return max(0.0, min(1.0, 0.5 - dist))


def pixel(x, y):
    px, py = x + 0.5 - SIZE / 2, y + 0.5 - SIZE / 2
    body = coverage(sd_round_rect(px, py, BODY / 2, RADIUS))
    if body == 0.0:
        return (0, 0, 0, 0)
    # The hexagon is drawn with flat sides left and right, so rotate 90 degrees.
    ring = coverage(abs(sd_hexagon(py, px, HEX_R)) - STROKE / 2)
    r, g, b = (round(f + (gl - f) * ring) for f, gl in zip(FILL, GLYPH))
    return (r, g, b, round(255 * body))


def png(width, height, rows):
    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\x00" + bytes(row) for row in rows)
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9))
            + chunk(b"IEND", b""))


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "AppIcon-1024.png"
    rows = []
    for y in range(SIZE):
        row = bytearray()
        for x in range(SIZE):
            row.extend(pixel(x, y))
        rows.append(row)
    with open(out, "wb") as f:
        f.write(png(SIZE, SIZE, rows))
    print("wrote", out)


if __name__ == "__main__":
    main()
