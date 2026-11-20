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

## Native runtimes (call translation)

The call-translation build links two native runtimes as dynamic libraries
(`libprxvc_qwentts.dylib`, `libprxvc_transcribe.dylib`, placed in
`Contents/Frameworks`). Their sources are vendored unmodified under
`crates/native/*/vendor/` with their licence files; `VENDOR.toml` next to each
records the upstream URL and the pinned commit. Every component below is MIT
licensed; the licence file of each component is named in its entry. The
copyright notices are reproduced here verbatim, followed by the MIT permission
notice that applies to all MIT components in this section.

### qwentts.cpp (text to speech)

- Used for: speech synthesis in the call-translation pipeline.
- Source: https://github.com/ServeurpersoCom/qwentts.cpp, commit
  2edb35f87b709a070b2d5379f77e5c52a1daa585 (2026-11-17).
- Licence: MIT (`crates/native/qwentts-sys/vendor/qwentts.cpp/LICENSE`).
- Copyright: Copyright (c) 2023-2026 The omnivoice.cpp authors

### ggml, copy inside qwentts.cpp

- Source: https://github.com/ServeurpersoCom/ggml, commit
  ef4555b7aede4467a5de803b103615e0a5f30dd8 (submodule of qwentts.cpp).
- Licence: MIT (`crates/native/qwentts-sys/vendor/qwentts.cpp/ggml/LICENSE`).
- Copyright: Copyright (c) 2023-2026 The ggml authors

### transcribe.cpp (speech recognition)

- Used for: speech recognition and voice activity detection.
- Source: https://github.com/handy-computer/transcribe.cpp, commit
  275cd50d2c4104c204009a197ac091b9687668e5 (2026-11-11).
- Licence: MIT (`crates/native/transcribe-sys/vendor/transcribe.cpp/LICENSE`;
  upstream's own list of its third-party components is
  `vendor/transcribe.cpp/THIRD-PARTY-LICENSES.md`).
- Copyright: Copyright (c) 2026 The transcribe.cpp authors

### ggml, copy inside transcribe.cpp

- Source: https://github.com/ggml-org/ggml, commit
  d7cb574130e6f01ad25b3289685489200febcd74, with the four downstream patches
  listed in `vendor/transcribe.cpp/ggml/UPSTREAM`.
- Licence: MIT (`crates/native/transcribe-sys/vendor/transcribe.cpp/ggml/LICENSE`).
- Copyright: Copyright (c) 2023-2026 The ggml authors

### Silero VAD (translation inside transcribe.cpp)

- Source: https://github.com/snakers4/silero-vad, version 6.2.3 (tag v6.2.3,
  commit 5cd7945). The code is a translation of its speech-timestamp
  post-processing; no model or Python source is included.
- Licence: MIT (`vendor/transcribe.cpp/src/third_party/silero_vad/LICENSE`).
- Copyright: Copyright (c) 2020-present Silero Team

### MIT permission notice (qwentts.cpp, transcribe.cpp, ggml, Silero VAD)

```

## Virtual microphone driver

The macOS virtual microphone driver (`driver/macos/PrxVoiceMicrophoneDriver.c`, built as
`PrxVoiceMicrophone.driver` and carried in the app bundle under
`Contents/Resources/Drivers/`) is adapted from the MIT-licensed Hex microphone
driver. The same text is in `driver/macos/NOTICE` and inside the driver bundle
(`Contents/Resources/NOTICE`).

### Hex microphone driver

- Used for: the audio server plug-in that exposes "PRX Voice Microphone".
- Source: `HexMicrophoneDriver.c` of the Hex project (https://github.com/anomalyco/hex).
- Licence: MIT.
- Copyright: Copyright (c) 2026 Kit Langton; Copyright (c) 2026 Christopher
  Langton (the name given in the Hex repository LICENSE file).
- Modification: identifiers, device names, device and box UIDs, manufacturer
  string, bundle identifier and factory UUID replaced; the reported transport
  type is a build-time option. The driver logic is otherwise unchanged.

```text
MIT License

Copyright (c) 2026 Kit Langton
Copyright (c) 2026 Christopher Langton (as named in the Hex repository LICENSE)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### miniz (inside transcribe.cpp)

- Source: https://github.com/richgel999/miniz, release 3.1.1 (commit
  d10b03cc73475af673df40f06e5cefd1d5f940d9).
- Licence: MIT (`vendor/transcribe.cpp/src/third_party/miniz/LICENSE`),
  reproduced in full below.

```
Copyright 2013-2014 RAD Game Tools and Valve Software
Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC

All Rights Reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

