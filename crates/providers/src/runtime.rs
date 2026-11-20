//! The runtimes a catalogue model can name, and which of them this binary contains.

/// A runtime named by a catalogue entry (`runtime.name`), parsed once.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Runtime {
    /// An external `llama-server` reached over HTTP; nothing is linked in.
    LlamaServer,
    /// In-process `qwentts.cpp` (cargo feature `qwentts`).
    QwenTts,
    /// In-process `transcribe.cpp`: recognition and VAD (cargo feature `transcribe`).
    Transcribe,
    /// The retired `whisper.cpp` binding (removed in CT-20); no call role uses it.
    WhisperCpp,
    /// ONNX Runtime, for the light TTS fallback; no adapter exists yet.
    OnnxRuntime,
    /// A runtime name this client does not know.
    Other(String),
}

impl Runtime {
    /// Parse the catalogue's runtime name.
    pub fn parse(name: &str) -> Self {
        match name {
            "llama-server" => Self::LlamaServer,
            "qwentts.cpp" => Self::QwenTts,
            "transcribe" | "transcribe.cpp" => Self::Transcribe,
            "whisper.cpp" => Self::WhisperCpp,
            "onnxruntime" => Self::OnnxRuntime,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The catalogue's name for it.
    pub fn name(&self) -> &str {
        match self {
            Self::LlamaServer => "llama-server",
            Self::QwenTts => "qwentts.cpp",
            Self::Transcribe => "transcribe.cpp",
            Self::WhisperCpp => "whisper.cpp",
            Self::OnnxRuntime => "onnxruntime",
            Self::Other(n) => n,
        }
    }

    /// Whether the binary described by `built` can run it.
    pub fn is_built_in(&self, built: &BuiltRuntimes) -> bool {
        match self {
            Self::LlamaServer => true,
            Self::QwenTts => built.qwentts,
            Self::Transcribe => built.transcribe,
            Self::WhisperCpp | Self::OnnxRuntime | Self::Other(_) => false,
        }
    }
}

/// Which in-process runtimes this binary contains. The registry derives it from its loaders, so
/// a loader that cannot load (feature off) makes the runtime a typed problem at resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltRuntimes {
    /// `qwentts.cpp`.
    pub qwentts: bool,
    /// `transcribe.cpp` and its recognition adapters.
    pub transcribe: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_unknown_runtimes_are_kept() {
        for name in [
            "llama-server",
            "qwentts.cpp",
            "transcribe.cpp",
            "whisper.cpp",
            "onnxruntime",
        ] {
            assert_eq!(Runtime::parse(name).name(), name);
        }
        assert_eq!(
            Runtime::parse("future.cpp"),
            Runtime::Other("future.cpp".into())
        );
        let none = BuiltRuntimes {
            qwentts: false,
            transcribe: false,
        };
        assert!(!Runtime::parse("future.cpp").is_built_in(&none));
        assert!(Runtime::LlamaServer.is_built_in(&none));
        assert!(Runtime::QwenTts.is_built_in(&BuiltRuntimes {
            qwentts: true,
            ..none
        }));
    }
}
