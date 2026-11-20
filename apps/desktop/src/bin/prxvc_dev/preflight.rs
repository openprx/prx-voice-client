//! `prxvc-dev preflight`: print what a translated call needs and whether it is in place. Changes nothing.

use crate::call_kit::config::load_current;
use crate::call_kit::readiness::{Line, Status, machine_lines, provider_lines, render};

/// Print the findings; `true` when nothing is missing.
pub(crate) fn run() -> Result<bool, String> {
    let config = load_current();
    let mut lines: Vec<Line> = Vec::new();
    #[cfg(feature = "qwentts")]
    lines.push(Line {
        status: Status::Ok,
        label: "native runtime".into(),
        detail: format!(
            "qwentts {}",
            prxvc_adapters::tts::qwentts::engine::runtime_version()
        ),
    });
    #[cfg(feature = "transcribe")]
    lines.push(Line {
        status: Status::Ok,
        label: "native runtime".into(),
        detail: format!(
            "transcribe {}",
            prxvc_adapters::transcribe::runtime_version()
        ),
    });
    #[cfg(not(all(feature = "qwentts", feature = "transcribe")))]
    lines.push(Line {
        status: Status::Missing,
        label: "native runtime".into(),
        detail: "this helper was built without the qwentts and transcribe features: rebuild with --features \"qwentts transcribe\"".into(),
    });
    lines.extend(machine_lines(&config));
    lines.extend(provider_lines(&config));
    println!("{}", render(&lines));
    let missing = lines.iter().filter(|l| l.status == Status::Missing).count();
    if missing == 0 {
        println!("everything for a translated call is in place");
    } else {
        println!("{missing} item(s) missing");
    }
    Ok(missing == 0)
}
