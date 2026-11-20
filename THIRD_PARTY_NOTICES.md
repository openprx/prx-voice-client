# Third-party notices

## Bundled fonts

The desktop client embeds the fonts below (crates/desktop-ui/assets/fonts/).
All three are licensed under the SIL Open Font License, Version 1.1; the full
text of each licence is next to the fonts (`OFL-*.txt`). The fonts are not sold
by themselves. The files are produced by `scripts/build-fonts.sh`, which
documents every transformation.

### Hanken Grotesk

- Used for: UI text (weights 400, 500, 600).
- Source: https://github.com/marcologous/hanken-grotesk, taken from
  https://github.com/google/fonts (ofl/hankengrotesk, commit
  869789e84da92782c144d83d69b64cec54905b5f), version 3.013.
- Licence: SIL OFL 1.1 (`OFL-HankenGrotesk.txt`). No Reserved Font Name.
- Copyright: Copyright 2021 The Hanken Grotesk Project Authors
  (https://github.com/marcologous/hanken-grotesk)
- Modification: the variable font was instantiated as static fonts at weights
  400, 500 and 600 (fontTools). No glyphs were removed.

### JetBrains Mono

- Used for: monospace text (weights 400 and 600).
- Source: https://github.com/JetBrains/JetBrainsMono, release v2.304
  (tag commit cd5227bd1f61dff3bbd6c814ceaf7ffd95e947d9), files
  `fonts/ttf/JetBrainsMono-Regular.ttf` and `JetBrainsMono-SemiBold.ttf`.
- Licence: SIL OFL 1.1 (`OFL-JetBrainsMono.txt`). No Reserved Font Name.
- Copyright: Copyright 2020 The JetBrains Mono Project Authors
  (https://github.com/JetBrains/JetBrainsMono)
- Modification: none.

### Noto Sans SC

- Used for: Chinese text (weights 400 and 600).
- Source: https://github.com/notofonts/noto-cjk (tag Sans2.004, commit
  523d033d6cb47f4a80c58a35753646f5c3608a78), taken from
  https://github.com/google/fonts (ofl/notosanssc, commit
  869789e84da92782c144d83d69b64cec54905b5f), version 2.004.
- Licence: SIL OFL 1.1 (`OFL-NotoSansSC.txt`).
- Copyright: Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved
  Font Name 'Source'. Source is a trademark of Adobe in the United States and/or
  other countries.
- Modification: the variable font was subset (Basic Latin, all GB2312
  characters, CJK Symbols and Punctuation, Halfwidth and Fullwidth Forms,
  General Punctuation dashes, quotes and ellipsis; 7,725 code points) and
  instantiated as static fonts at weights 400 and 600 (fontTools). The
  Reserved Font Name 'Source' is not used as, or in, the name of the modified
  fonts, which keep the family name "Noto Sans SC".
