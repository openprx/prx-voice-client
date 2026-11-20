//! Speech synthesis adapters implementing the `prxvc-ports` TTS port.
//!
//! * [`qwentts`]: the in-process `qwentts.cpp` engine (`QwenTts`). Everything
//!   except the raw FFI wrapper builds without the native library; the
//!   wrapper (`qwentts::engine`) is behind the cargo feature `qwentts`.

pub mod qwentts;
