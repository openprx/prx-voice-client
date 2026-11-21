//! When the UI repaints (egui upgrade decision D5).
//!
//! egui repaints on input, on its own animations and on explicit requests;
//! left alone an idle app draws nothing. The orb has a slow breathing
//! animation, so the app asks for repaints itself, as rarely as the state
//! allows. [`decide`] is a pure function from what is on screen to a
//! [`Repaint`]; [`schedule`] hands the result to egui.
//!
//! Changes made by other threads wake the UI through
//! [`RepaintSignal`](crate::shared::RepaintSignal), and input in a tool
//! window repaints the parent viewport inside eframe, so neither needs a
//! periodic repaint here.

use std::time::Duration;

/// Repaint interval while the orb only breathes: about 30 frames per second.
pub const IDLE_REPAINT_INTERVAL: Duration = Duration::from_millis(33);

/// Safety-net interval while a tool window is open and the orb needs no
/// animation. The windows are immediate viewports drawn inside the parent's
/// pass, so they only update when the parent repaints; egui's own input and
/// animation requests and the shared-state wake-ups cover almost everything,
/// this keeps the rest (for example a window that was resized by the OS)
/// from stalling.
pub const TOOL_WINDOW_REPAINT_INTERVAL: Duration = Duration::from_millis(100);

/// What the policy asks egui to do at the end of a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repaint {
    /// Repaint every frame (as fast as the display refreshes).
    Now,
    /// Repaint once after this delay.
    After(Duration),
    /// Schedule nothing; wait for input or a wake-up.
    Never,
}

impl Repaint {
    /// The more urgent of two requests.
    fn soonest(self, other: Self) -> Self {
        match (self, other) {
            (Self::Now, _) | (_, Self::Now) => Self::Now,
            (Self::After(a), Self::After(b)) => Self::After(a.min(b)),
            (Self::After(a), Self::Never) | (Self::Never, Self::After(a)) => Self::After(a),
            (Self::Never, Self::Never) => Self::Never,
        }
    }

    /// Upper bound of frames per second this request produces on a display
    /// refreshing at `refresh_hz` (a `Now` request is paced by the display).
    pub fn max_frames_per_second(self, refresh_hz: f64) -> f64 {
        match self {
            Self::Now => refresh_hz,
            Self::After(d) => (1.0 / d.as_secs_f64()).min(refresh_hz),
            Self::Never => 0.0,
        }
    }
}

/// What is on screen, as far as the repaint policy cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepaintInputs<'a> {
    /// Session state label, as in `DesktopUiState::session_state`.
    pub session_state: &'a str,
    /// The orb window is hidden or minimised.
    pub orb_hidden: bool,
    /// At least one tool window (settings, history, debug) is open.
    pub tool_window_open: bool,
    /// The idle breathing animation is enabled (tests turn it off).
    pub idle_animation: bool,
}

/// States in which the orb reacts to speech or a reply is being produced:
/// its animation is faster and is drawn every frame. `Listening` only
/// breathes, like idle, and uses the idle rate.
pub fn is_active_state(session_state: &str) -> bool {
    matches!(
        session_state,
        "Starting" | "UserSpeaking" | "Recognizing" | "Thinking" | "AssistantSpeaking"
    )
}

/// The repaint policy.
pub fn decide(inputs: &RepaintInputs<'_>) -> Repaint {
    let orb = if inputs.orb_hidden {
        Repaint::Never
    } else if is_active_state(inputs.session_state) {
        Repaint::Now
    } else if inputs.idle_animation {
        Repaint::After(IDLE_REPAINT_INTERVAL)
    } else {
        Repaint::Never
    };
    let tools = if inputs.tool_window_open {
        Repaint::After(TOOL_WINDOW_REPAINT_INTERVAL)
    } else {
        Repaint::Never
    };
    orb.soonest(tools)
}

/// Apply a decision to egui.
pub fn schedule(ctx: &egui::Context, repaint: Repaint) {
    match repaint {
        Repaint::Now => ctx.request_repaint(),
        Repaint::After(delay) => ctx.request_repaint_after(delay),
        Repaint::Never => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(state: &str) -> RepaintInputs<'_> {
        RepaintInputs {
            session_state: state,
            orb_hidden: false,
            tool_window_open: false,
            idle_animation: true,
        }
    }

    #[test]
    fn idle_repaints_at_about_30_fps() {
        for state in ["", "Idle", "Completed", "Failed", "Listening"] {
            let r = decide(&inputs(state));
            assert_eq!(r, Repaint::After(IDLE_REPAINT_INTERVAL), "{state}");
            let fps = r.max_frames_per_second(60.0);
            assert!((29.0..=31.0).contains(&fps), "{state}: {fps}");
        }
        assert_eq!(IDLE_REPAINT_INTERVAL, Duration::from_millis(33));
    }

    #[test]
    fn speech_and_reply_states_repaint_every_frame() {
        for state in ["UserSpeaking", "Recognizing", "Thinking", "AssistantSpeaking"] {
            assert_eq!(decide(&inputs(state)), Repaint::Now, "{state}");
        }
        assert_eq!(Repaint::Now.max_frames_per_second(60.0), 60.0);
        assert_eq!(Repaint::Now.max_frames_per_second(120.0), 120.0);
    }

    #[test]
    fn hidden_orb_without_tool_windows_schedules_nothing() {
        for state in ["Idle", "Listening", "AssistantSpeaking"] {
            let r = decide(&RepaintInputs { orb_hidden: true, ..inputs(state) });
            assert_eq!(r, Repaint::Never, "{state}");
            assert_eq!(r.max_frames_per_second(60.0), 0.0);
        }
    }

    #[test]
    fn tool_window_keeps_the_parent_repainting() {
        let open = |state, hidden| RepaintInputs {
            tool_window_open: true,
            orb_hidden: hidden,
            ..inputs(state)
        };
        // Visible orb: the faster of the orb and the window wins.
        assert_eq!(decide(&open("Idle", false)), Repaint::After(IDLE_REPAINT_INTERVAL));
        assert_eq!(decide(&open("Thinking", false)), Repaint::Now);
        // Hidden orb: only the window's safety net remains.
        let r = decide(&open("Idle", true));
        assert_eq!(r, Repaint::After(TOOL_WINDOW_REPAINT_INTERVAL));
        assert!((r.max_frames_per_second(60.0) - 10.0).abs() < 0.01);
    }

    #[test]
    fn disabled_idle_animation_stops_the_idle_repaints() {
        let off = RepaintInputs { idle_animation: false, ..inputs("Idle") };
        assert_eq!(decide(&off), Repaint::Never);
        // Active states still animate.
        assert_eq!(
            decide(&RepaintInputs { idle_animation: false, ..inputs("Thinking") }),
            Repaint::Now
        );
        // A tool window still gets its safety net.
        assert_eq!(
            decide(&RepaintInputs { tool_window_open: true, ..off }),
            Repaint::After(TOOL_WINDOW_REPAINT_INTERVAL)
        );
    }

    #[test]
    fn frames_per_second_by_state_at_60_hz() {
        // Documented numbers: idle ~30, active 60, hidden 0, tool window only 10.
        let fps = |i: RepaintInputs<'_>| decide(&i).max_frames_per_second(60.0).round();
        assert_eq!(fps(inputs("Idle")), 30.0);
        assert_eq!(fps(inputs("AssistantSpeaking")), 60.0);
        assert_eq!(fps(RepaintInputs { orb_hidden: true, ..inputs("Idle") }), 0.0);
        assert_eq!(
            fps(RepaintInputs { orb_hidden: true, tool_window_open: true, ..inputs("Idle") }),
            10.0
        );
    }
}
