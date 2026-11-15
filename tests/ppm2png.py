"""The screens of a GUI run as PNG files: FROM/NAME-LABEL.ppm (Zoraya's test runner keeps
them so) becomes TO/NAME.png at scale 1 and TO/NAME@2x.png at scale 2 (LABEL ends in the
scale).

Usage: ppm2png.py FROM TO LABEL
"""
import pathlib
import struct
import sys
import zlib


def png(width, height, rgb):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    rows = b"".join(b"\0" + rgb[y * width * 3:(y + 1) * width * 3] for y in range(height))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def read_ppm(path):
    data = path.read_bytes()
    fields, at = [], 0
    while len(fields) < 4:
        end = at
        while not data[end:end + 1].isspace():
            end += 1
        fields.append(data[at:end])
        at = end + 1
    if fields[0] != b"P6" or fields[3] != b"255":
        raise SystemExit(f"{path}: not an 8-bit binary PPM")
    return int(fields[1]), int(fields[2]), data[at:]


def main():
    source, target, label = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3]
    scale = label.rsplit("-", 1)[1]
    target.mkdir(parents=True, exist_ok=True)
    found = sorted(source.glob(f"*-{label}.ppm"))
    if not found:
        raise SystemExit(f"no screens of {label} in {source}")
    for ppm in found:
        name = ppm.name[: -len(f"-{label}.ppm")]
        width, height, rgb = read_ppm(ppm)
        out = target / (f"{name}.png" if scale == "1" else f"{name}@{scale}x.png")
        out.write_bytes(png(width, height, rgb))
        print(f"shots: {out} ({width} x {height})")


if __name__ == "__main__":
    main()
