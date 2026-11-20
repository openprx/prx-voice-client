"""Build the static, subset font files bundled into the desktop UI.

Invoked by scripts/build-fonts.sh (do not run it directly).

egui renders only the default instance of a font, so variable fonts are
turned into static instances here:

  Hanken Grotesk   wght 400, 500, 600   (full glyph set, no subsetting)
  JetBrains Mono   Regular, SemiBold    (copied from the release, unchanged)
  Noto Sans SC     wght 400, 600        (subset, see CHARSET below)

Noto Sans SC subset coverage:
  - Basic Latin U+0020-007E
  - every character of GB2312 (level 1 and level 2 hanzi, 6763 in total,
    plus its 682 symbols, kana, Greek, Cyrillic, pinyin and box drawing)
  - CJK Symbols and Punctuation U+3000-303F
  - Halfwidth and Fullwidth Forms U+FF00-FFEF
  - General Punctuation U+2010-2027, U+2030-203B (dashes, quotes, ellipsis)
Everything else (rare hanzi, Traditional-only forms, other scripts) is left to
the system CJK font fallback.

Licence: Noto Sans SC is OFL-1.1 with Reserved Font Name 'Source'. The subset
keeps the name "Noto Sans SC" (no reserved name is used) and the original
copyright and licence name records.
"""
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer


def gb2312_chars():
    out = set()
    # Lead bytes 0xA1-0xF7, trail bytes 0xA1-0xFE.
    for hi in range(0xA1, 0xF8):
        for lo in range(0xA1, 0xFF):
            try:
                out.add(bytes([hi, lo]).decode("gb2312"))
            except UnicodeDecodeError:
                pass
    return out


def codepoints():
    cps = {ord(c) for c in gb2312_chars()}
    cps.update(range(0x20, 0x7F))
    cps.update(range(0x3000, 0x3040))
    cps.update(range(0xFF00, 0xFFF0))
    cps.update(range(0x2010, 0x2028))
    cps.update(range(0x2030, 0x203C))
    return sorted(cps)


def instance(path, wght, out):
    font = TTFont(path)
    inst = instancer.instantiateVariableFont(font, {"wght": wght}, updateFontNames=True)
    inst.save(out)


def main():
    work, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)

    for wght, label in ((400, "Regular"), (500, "Medium"), (600, "SemiBold")):
        instance(work / "HankenGrotesk-wght.ttf", wght, out / f"HankenGrotesk-{label}.ttf")

    for label in ("Regular", "SemiBold"):
        src = work / "jbm" / "fonts" / "ttf" / f"JetBrainsMono-{label}.ttf"
        (out / f"JetBrainsMono-{label}.ttf").write_bytes(src.read_bytes())

    cps = codepoints()
    print(f"Noto Sans SC subset: {len(cps)} code points requested")
    opts = subset.Options()
    opts.layout_features = ["kern", "vert", "vrt2", "locl"]
    opts.name_IDs = ["*"]
    opts.name_legacy = True
    opts.notdef_outline = True
    opts.hinting = False
    opts.glyph_names = False
    opts.drop_tables += ["DSIG"]
    font = TTFont(work / "NotoSansSC-wght.ttf")
    subsetter = subset.Subsetter(opts)
    subsetter.populate(unicodes=cps)
    subsetter.subset(font)
    cmap = font.getBestCmap()
    missing = [hex(c) for c in cps if c not in cmap]
    print(f"Noto Sans SC subset: {len(cmap)} mapped, {len(missing)} requested but absent in the source")
    if missing:
        print("absent:", " ".join(missing[:40]), "..." if len(missing) > 40 else "")
    sub_path = work / "NotoSansSC-subset-wght.ttf"
    font.save(sub_path)
    for wght, label in ((400, "Regular"), (600, "SemiBold")):
        instance(sub_path, wght, out / f"NotoSansSC-{label}.ttf")


main()
