//! Everything a call needs, resolved at once: the full set for both directions or the complete
//! list of problems.

use prxvc_media::{Direction, Lang};

use crate::keys::{BadLanguage, CallLanguages, other, parse_lang};
use crate::problem::{Problem, ProblemKind, Slot};
use crate::resolve::{Resolver, VoiceChoice};
use crate::spec::{LocalModelSpec, MtSpec, Resolved, TtsSpec};

/// How recognition is arranged (`pipeline.recognition_mode`, PD-2.5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecognitionMode {
    /// Streaming partials and the streaming final.
    Streaming,
    /// Streaming partials, a refine decode of each committed clause.
    TwoPass,
    /// A refine-class model that re-decodes a growing buffer behind the streaming port; for the
    /// registry it needs the same `redecode` model as the other modes.
    Segmented,
}

impl RecognitionMode {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "streaming" => Some(Self::Streaming),
            "two_pass" => Some(Self::TwoPass),
            "segmented" => Some(Self::Segmented),
            _ => None,
        }
    }
}

/// The providers of one direction.
#[derive(Clone, Debug)]
pub struct DirectionPlan {
    /// The direction.
    pub direction: Direction,
    /// Language spoken.
    pub source: Lang,
    /// Language produced.
    pub target: Lang,
    /// Recognition of `source`.
    pub asr: Resolved<LocalModelSpec>,
    /// The refine pass of `source`; present in `two_pass` mode.
    pub refine: Option<Resolved<LocalModelSpec>>,
    /// Translation `source` to `target`.
    pub mt: Resolved<MtSpec>,
    /// Synthesis of `target` with this direction's voice.
    pub tts: Resolved<TtsSpec>,
}

/// The resolved set for a call.
#[derive(Clone, Debug)]
pub struct CallPlan {
    /// The two languages.
    pub languages: CallLanguages,
    /// The recognition arrangement.
    pub recognition: RecognitionMode,
    /// The voice activity detector; each direction opens its own engine from it.
    pub vad: Resolved<LocalModelSpec>,
    /// Local user to remote party.
    pub outgoing: DirectionPlan,
    /// Remote party to local user.
    pub incoming: DirectionPlan,
    /// Problems of fallbacks. Non-fatal: every primary resolved.
    pub warnings: Vec<Problem>,
}

/// The roles of the directions asked for, resolved at once.
#[derive(Clone, Debug)]
pub struct DirectionsPlan {
    /// The two call languages.
    pub languages: CallLanguages,
    /// How recognition runs.
    pub recognition: RecognitionMode,
    /// The voice activity detector, shared by the directions.
    pub vad: Resolved<LocalModelSpec>,
    /// The directions asked for, in the order asked.
    pub directions: Vec<DirectionPlan>,
    /// Non-fatal findings (a fallback that cannot be used).
    pub warnings: Vec<Problem>,
}

impl CallPlan {
    /// The plan of `direction`.
    pub fn direction(&self, direction: Direction) -> &DirectionPlan {
        match direction {
            Direction::Outgoing => &self.outgoing,
            Direction::Incoming => &self.incoming,
        }
    }
}

fn call_problem(kind: ProblemKind) -> Problem {
    Problem {
        role: None,
        slot: Slot::Primary,
        kind,
    }
}

/// Collects results and problems without stopping at the first failure.
struct Collect {
    problems: Vec<Problem>,
    warnings: Vec<Problem>,
}

impl Collect {
    fn take<S>(&mut self, result: Result<Resolved<S>, Vec<Problem>>) -> Option<Resolved<S>> {
        match result {
            Ok(mut resolved) => {
                self.warnings.append(&mut resolved.warnings);
                Some(resolved)
            }
            Err(mut problems) => {
                self.problems.append(&mut problems);
                None
            }
        }
    }
}

fn dedupe(problems: Vec<Problem>) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::with_capacity(problems.len());
    for p in problems {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

impl Resolver<'_> {
    /// The two call languages: configured, else derived (own from the interface language or the
    /// system language, peer as the other language).
    pub(crate) fn languages(&self, system: Option<Lang>) -> Result<CallLanguages, Vec<Problem>> {
        let bad = |b: BadLanguage| {
            call_problem(ProblemKind::InvalidLanguage {
                key: b.key.to_owned(),
                value: b.value,
            })
        };
        let own = parse_lang("call.self_language", &self.cfg.call.self_language)
            .map_err(|b| vec![bad(b)])?;
        let peer = parse_lang("call.peer_language", &self.cfg.call.peer_language)
            .map_err(|b| vec![bad(b)])?;
        let ui = match self.cfg.ui.language.trim() {
            "" | "system" => system,
            text => parse_lang("ui.language", text).ok().flatten().or(system),
        };
        match (own.or(ui), peer) {
            (Some(own), Some(peer)) => Ok(CallLanguages { own, peer }),
            (Some(own), None) => Ok(CallLanguages {
                own,
                peer: other(own),
            }),
            (None, Some(peer)) => Ok(CallLanguages {
                own: other(peer),
                peer,
            }),
            (None, None) => Err(vec![call_problem(ProblemKind::LanguageNotSet {
                key: "call.self_language".to_owned(),
            })]),
        }
    }

    /// The voice an outgoing or incoming direction asks for.
    fn voice_choice(&self, direction: Direction) -> VoiceChoice {
        match direction {
            Direction::Outgoing if self.cfg.voice.use_clone_outgoing => VoiceChoice::OwnClone,
            Direction::Outgoing => VoiceChoice::Preset(self.cfg.voice.fallback_preset.clone()),
            Direction::Incoming => VoiceChoice::Preset(self.cfg.voice.incoming_preset.clone()),
        }
    }

    /// Resolve everything, collecting every problem.
    pub(crate) fn call(&self, system: Option<Lang>) -> Result<CallPlan, Vec<Problem>> {
        let plan = self.directions(system, &[Direction::Outgoing, Direction::Incoming])?;
        let mut outgoing = None;
        let mut incoming = None;
        for d in plan.directions {
            match d.direction {
                Direction::Outgoing => outgoing = Some(d),
                Direction::Incoming => incoming = Some(d),
            }
        }
        match (outgoing, incoming) {
            (Some(outgoing), Some(incoming)) => Ok(CallPlan {
                languages: plan.languages,
                recognition: plan.recognition,
                vad: plan.vad,
                outgoing,
                incoming,
                warnings: plan.warnings,
            }),
            _ => unreachable!("both directions were asked for"),
        }
    }

    /// Resolve the voice activity detector and the roles of `wanted` directions only.
    pub(crate) fn directions(
        &self,
        system: Option<Lang>,
        wanted: &[Direction],
    ) -> Result<DirectionsPlan, Vec<Problem>> {
        let mut c = Collect {
            problems: Vec::new(),
            warnings: Vec::new(),
        };
        let mode_text = self.cfg.pipeline.recognition_mode.as_str();
        let mode = RecognitionMode::parse(mode_text).unwrap_or_else(|| {
            c.problems.push(call_problem(ProblemKind::InvalidSetting {
                key: "pipeline.recognition_mode".to_owned(),
                value: mode_text.to_owned(),
            }));
            RecognitionMode::Streaming
        });
        let vad = c.take(self.vad());
        let languages = match self.languages(system) {
            Ok(l) => l,
            Err(mut p) => {
                c.problems.append(&mut p);
                return Err(dedupe(c.problems));
            }
        };
        let this = self;
        let mut direction = |d: Direction| {
            let (source, target) = languages.pair(d);
            let asr = c.take(this.asr(source));
            let refine = match mode {
                RecognitionMode::TwoPass => Some(c.take(this.refine(source))),
                _ => None,
            };
            let mt = c.take(this.mt(source, target));
            let tts = c.take(this.tts(target, &this.voice_choice(d)));
            Some(DirectionPlan {
                direction: d,
                source,
                target,
                asr: asr?,
                refine: match refine {
                    None => None,
                    Some(r) => Some(r?),
                },
                mt: mt?,
                tts: tts?,
            })
        };
        let planned: Vec<Option<DirectionPlan>> = wanted.iter().map(|d| direction(*d)).collect();
        match vad {
            Some(vad) if c.problems.is_empty() && planned.iter().all(Option::is_some) => {
                Ok(DirectionsPlan {
                    languages,
                    recognition: mode,
                    vad,
                    directions: planned.into_iter().flatten().collect(),
                    warnings: dedupe(c.warnings),
                })
            }
            _ => {
                let mut all = c.problems;
                all.append(&mut c.warnings);
                Err(dedupe(all))
            }
        }
    }
}
