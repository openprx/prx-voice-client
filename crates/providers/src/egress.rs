//! What leaves the machine, per role, as plain data for the disclosure row (UX 4.8.3).

use crate::keys::RoleKey;
use crate::plan::CallPlan;
use crate::spec::{DataLeaving, Locality, MtSpec, Resolved};
use prxvc_ports::ProviderId;

/// One row of the disclosure: what a role's provider receives and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleEgress {
    /// The role and key.
    pub role: RoleKey,
    /// The provider.
    pub provider: ProviderId,
    /// What it receives.
    pub data: DataLeaving,
    /// The host it is sent to; `None` when nothing leaves the process.
    pub host: Option<String>,
    /// Where the host is.
    pub locality: Locality,
}

/// The disclosure of a resolved call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EgressSummary {
    /// One row per role and key, primaries first, then the fallbacks.
    pub rows: Vec<RoleEgress>,
}

impl EgressSummary {
    /// Whether anything is sent beyond this computer (a LAN host counts).
    pub fn leaves_this_computer(&self) -> bool {
        self.rows
            .iter()
            .any(|r| r.data != DataLeaving::Nothing && r.locality != Locality::ThisComputer)
    }

    /// The rows that send `data`.
    pub fn rows_sending(&self, data: DataLeaving) -> impl Iterator<Item = &RoleEgress> {
        self.rows.iter().filter(move |r| r.data == data)
    }
}

fn local_row(role: RoleKey, provider: &ProviderId) -> RoleEgress {
    RoleEgress {
        role,
        provider: provider.clone(),
        data: DataLeaving::Nothing,
        host: None,
        locality: Locality::ThisComputer,
    }
}

fn mt_row(role: RoleKey, spec: &MtSpec) -> RoleEgress {
    RoleEgress {
        role,
        provider: spec.id.clone(),
        data: spec.data_leaving(),
        host: Some(spec.host.clone()),
        locality: spec.locality,
    }
}

fn pair<S>(r: &Resolved<S>) -> impl Iterator<Item = &S> {
    std::iter::once(&r.primary).chain(r.fallback.as_ref())
}

/// The disclosure of `plan`.
pub fn summarize(plan: &CallPlan) -> EgressSummary {
    let mut primaries = Vec::new();
    let mut fallbacks = Vec::new();
    let mut push = |slot_fallback: bool, row: RoleEgress| {
        let rows = if slot_fallback { &mut fallbacks } else { &mut primaries };
        if !rows.contains(&row) {
            rows.push(row);
        }
    };
    for (i, spec) in pair(&plan.vad).enumerate() {
        push(i > 0, local_row(RoleKey::Vad, &spec.id));
    }
    for d in [&plan.outgoing, &plan.incoming] {
        for (i, spec) in pair(&d.asr).enumerate() {
            push(i > 0, local_row(RoleKey::Asr(d.source), &spec.id));
        }
        if let Some(refine) = &d.refine {
            for (i, spec) in pair(refine).enumerate() {
                push(i > 0, local_row(RoleKey::AsrRefine(d.source), &spec.id));
            }
        }
        let key = RoleKey::Mt {
            source: d.source,
            target: d.target,
        };
        for (i, spec) in pair(&d.mt).enumerate() {
            push(i > 0, mt_row(key, spec));
        }
        for (i, spec) in pair(&d.tts).enumerate() {
            push(i > 0, local_row(RoleKey::Tts(d.target), &spec.id));
        }
    }
    primaries.extend(fallbacks);
    EgressSummary { rows: primaries }
}
