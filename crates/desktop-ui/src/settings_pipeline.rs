//! Settings page Speech pipeline: the provider the registry resolves for each role, what leaves the machine, and every problem (design 2.2 and 4.8; read-only).

use egui::Ui;

use crate::settings_command::{RevealTarget, SettingsCommand};
use crate::settings_data::{
    DataLeavingView, LocalityView, PipelineData, PipelineProblem, PipelineRole, PipelineRow,
};
use crate::settings_rows::{
    PageCx, body_medium, note, notice, saved_only, select_row, status_row, switch_row, t, text_row,
    unavailable_row,
};
use crate::settings_view::{index_of, is_http_url, labels};
use crate::theme::space;
use crate::widgets::{self, Tone};

/// Values of `models.endpoint` for the first two options; any other value is a custom URL.
const SOURCES: [&str; 2] = ["huggingface", "modelscope"];
/// Values of `models.max_concurrent`.
const CONCURRENT: [u32; 3] = [1, 2, 3];

fn restart_or_download(description: &str) -> String {
    format!("{description} {}", t("settings.note.next_download"))
}

/// The label of a role row: role and key, for example "Translation · Chinese → English".
pub(crate) fn row_label(role: PipelineRole, key: &str) -> String {
    let lang = |code: &str| {
        let k = format!("settings.lang.{code}");
        let text = prxvc_i18n::t(&k);
        if text == k { code.to_owned() } else { text }
    };
    let role_text = t(match role {
        PipelineRole::Vad => "settings.pipeline.role.vad",
        PipelineRole::Recognition => "settings.pipeline.role.asr",
        PipelineRole::Refine => "settings.pipeline.role.refine",
        PipelineRole::Translation => "settings.pipeline.role.mt",
        PipelineRole::Voice => "settings.pipeline.role.tts",
        PipelineRole::Dialogue => "settings.pipeline.role.dialogue",
    });
    if key.is_empty() {
        return role_text;
    }
    let key_text = match key.split_once('>') {
        Some((from, to)) => format!("{} \u{2192} {}", lang(from), lang(to)),
        None => lang(key),
    };
    format!("{role_text} \u{b7} {key_text}")
}

/// The egress line of a row: where it runs or what is sent where (design 4.8.3).
pub(crate) fn egress_line(row: &PipelineRow) -> String {
    let host = row.host.as_deref().unwrap_or("");
    match (row.data, row.locality) {
        (DataLeavingView::Nothing, _) | (_, LocalityView::ThisComputer) => {
            t("settings.pipeline.egress.local")
        }
        (DataLeavingView::Audio, LocalityView::LocalNetwork) => {
            prxvc_i18n::t_with("settings.pipeline.egress.audio_lan", &[host])
        }
        (DataLeavingView::Text, LocalityView::LocalNetwork) => {
            prxvc_i18n::t_with("settings.pipeline.egress.text_lan", &[host])
        }
        (DataLeavingView::Audio, LocalityView::Remote) => {
            prxvc_i18n::t_with("settings.pipeline.egress.audio", &[host])
        }
        (DataLeavingView::Text, LocalityView::Remote) => {
            prxvc_i18n::t_with("settings.pipeline.egress.text", &[host])
        }
    }
}

/// The translated sentence for a problem, with the instruction it carries.
pub(crate) fn problem_text(p: &PipelineProblem) -> String {
    let key = format!("settings.pipeline.problem.{}", p.code);
    let args: Vec<&str> = p.args.iter().map(String::as_str).collect();
    let mut text = prxvc_i18n::t_with(&key, &args);
    if text == key {
        text = prxvc_i18n::t_with("settings.pipeline.problem.other", &[&p.code]);
    }
    match &p.subject {
        None => text,
        Some((role, key)) => format!("{}: {}", row_label(*role, key), text),
    }
}

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;

    // a. Setup preset.
    unavailable_row(
        ui,
        tk,
        &t("settings.pipeline.setup"),
        &t("settings.pipeline.setup.desc"),
        "settings.reason.presets",
        Some(&t(&format!(
            "settings.pipeline.preset.{}",
            cx.settings.providers_preset
        ))),
    );
    let (tone, summary) = leaving_summary(cx);
    status_row(
        ui,
        tk,
        &t("settings.pipeline.leaving"),
        &t("settings.pipeline.leaving.desc"),
        tone,
        &summary,
        None,
    );

    // b. Roles.
    widgets::section_header(ui, tk, &t("settings.pipeline.roles"), false);
    unavailable_row(
        ui,
        tk,
        &t("settings.pipeline.choose"),
        &t("settings.pipeline.choose.desc"),
        "settings.reason.provider_editor",
        None,
    );
    chain(ui, cx, &cx.data.pipeline.clone());

    // b2. The voice assistant's own chain: recognition and voice of the call, and its dialogue.
    widgets::section_header(ui, tk, &t("settings.pipeline.assistant"), false);
    note(ui, tk, &t("settings.pipeline.assistant.desc"));
    chain(ui, cx, &cx.data.assistant.chain.clone());

    // c. Cloud and server providers.
    widgets::section_header(ui, tk, &t("settings.pipeline.cloud"), false);
    note(ui, tk, &t("settings.pipeline.cloud.none"));
    unavailable_row(
        ui,
        tk,
        &t("settings.pipeline.add_provider"),
        &t("settings.pipeline.add_provider.desc"),
        "settings.reason.provider_editor",
        None,
    );

    // d. Local models.
    widgets::section_header(ui, tk, &t("settings.pipeline.models"), false);
    unavailable_row(
        ui,
        tk,
        &t("settings.pipeline.model_list"),
        &t("settings.pipeline.model_list.desc"),
        "settings.reason.model_manager",
        None,
    );

    // e. Storage and downloads.
    widgets::section_header(ui, tk, &t("settings.pipeline.storage"), false);
    storage_rows(ui, cx);
}

/// The rows of a resolved chain, or the problems that keep it from resolving.
fn chain(ui: &mut Ui, cx: &mut PageCx<'_>, data: &PipelineData) {
    let tk = cx.tk;
    match data {
        PipelineData::NotLoaded => {
            note(ui, tk, &t("settings.pipeline.loading"));
        }
        PipelineData::Problems(problems) => {
            body_medium(ui, tk, &t("settings.pipeline.problems"));
            ui.add_space(space::S8);
            for p in problems {
                notice(
                    ui,
                    tk,
                    if p.fallback {
                        Tone::Warning
                    } else {
                        Tone::Danger
                    },
                    &problem_text(p),
                );
                ui.add_space(space::S8);
            }
        }
        PipelineData::Resolved { rows, warnings, .. } => {
            for row in rows {
                role_row(ui, cx, row);
            }
            for p in warnings {
                notice(ui, tk, Tone::Warning, &problem_text(p));
                ui.add_space(space::S8);
            }
        }
    }
}

fn role_row(ui: &mut Ui, cx: &mut PageCx<'_>, row: &PipelineRow) {
    let mut text = format!("{} \u{b7} {}", row.provider, row.target);
    text.push('\n');
    text.push_str(&egress_line(row));
    if let Some(fallback) = &row.fallback {
        text.push('\n');
        text.push_str(&prxvc_i18n::t_with(
            "settings.pipeline.fallback",
            &[fallback],
        ));
    }
    status_row(
        ui,
        cx.tk,
        &row_label(row.role, &row.key),
        &text,
        Tone::Success,
        &t("settings.pipeline.ready"),
        None,
    );
}

fn leaving_summary(cx: &PageCx<'_>) -> (Tone, String) {
    if cx.settings.offline {
        return (Tone::Success, t("settings.pipeline.leaving.offline"));
    }
    match &cx.data.pipeline {
        PipelineData::Resolved {
            leaves_this_computer: false,
            ..
        } => (Tone::Success, t("settings.pipeline.leaving.nothing")),
        PipelineData::Resolved { rows, .. } => {
            let hosts: Vec<String> = rows
                .iter()
                .filter(|r| {
                    r.data != DataLeavingView::Nothing && r.locality != LocalityView::ThisComputer
                })
                .filter_map(|r| r.host.clone())
                .collect();
            (
                Tone::Warning,
                prxvc_i18n::t_with("settings.pipeline.leaving.some", &[&hosts.join(", ")]),
            )
        }
        _ => (Tone::Neutral, t("settings.pipeline.leaving.unknown")),
    }
}

fn storage_rows(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    switch_row(
        ui,
        tk,
        &t("settings.models.preload"),
        &saved_only(&t("settings.models.preload.desc")),
        &mut cx.settings.models_preload,
        true,
    );
    switch_row(
        ui,
        tk,
        &t("settings.models.warmup"),
        &saved_only(&t("settings.models.warmup.desc")),
        &mut cx.settings.models_warmup,
        true,
    );

    let custom = !SOURCES.contains(&cx.settings.models_endpoint.as_str());
    let mut options = labels(&["settings.models.source.hf", "settings.models.source.ms"]);
    options.push(t("settings.models.source.custom"));
    let selected = if custom {
        2
    } else {
        index_of(&SOURCES, &cx.settings.models_endpoint)
    };
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_models_source",
        &t("settings.models.source"),
        &restart_or_download(&t("settings.models.source.desc")),
        &options,
        selected,
    ) {
        match i {
            0 | 1 => cx.settings.models_endpoint = SOURCES[i].to_owned(),
            _ if !custom => cx.settings.models_endpoint = "https://".to_owned(),
            _ => {}
        }
    }
    if custom {
        let stored = cx.settings.models_endpoint.clone();
        if let Some(url) = text_row(
            ui,
            tk,
            cx.view,
            "models_endpoint",
            &t("settings.models.source.address"),
            &t("settings.models.source.custom.desc"),
            &stored,
            |v| {
                if v.trim().starts_with("https://") && is_http_url(v) {
                    Ok(())
                } else {
                    Err(t("settings.models.source.custom.invalid"))
                }
            },
        ) {
            cx.settings.models_endpoint = url.trim().to_owned();
        }
    }

    let options: Vec<String> = CONCURRENT.iter().map(u32::to_string).collect();
    let selected = CONCURRENT
        .iter()
        .position(|n| *n == cx.settings.models_max_concurrent)
        .unwrap_or(1);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_models_concurrent",
        &t("settings.models.concurrent"),
        &restart_or_download(&t("settings.models.concurrent.desc")),
        &options,
        selected,
    ) {
        cx.settings.models_max_concurrent = CONCURRENT[i];
    }

    let (path, chip) = if cx.data.about.models_dir.is_empty() {
        (
            t("settings.models.dir.unknown"),
            t("settings.models.dir.default"),
        )
    } else if cx.settings.models_dir.is_empty() {
        (
            cx.data.about.models_dir.clone(),
            t("settings.models.dir.default"),
        )
    } else {
        (
            cx.data.about.models_dir.clone(),
            t("settings.models.dir.custom"),
        )
    };
    let reveal = t(if cx.data.permissions.is_macos {
        "settings.about.reveal.finder"
    } else {
        "settings.about.reveal.files"
    });
    if status_row(
        ui,
        tk,
        &t("settings.models.dir"),
        &path,
        Tone::Neutral,
        &chip,
        Some((&reveal, !cx.data.about.models_dir.is_empty())),
    ) {
        cx.commands
            .push(SettingsCommand::Reveal(RevealTarget::ModelsDir));
    }
    unavailable_row(
        ui,
        tk,
        &t("settings.models.change_dir"),
        &t("settings.models.change_dir.desc"),
        "settings.reason.model_manager",
        None,
    );

    let stored = cx.settings.models_catalogue_url.clone();
    if let Some(url) = text_row(
        ui,
        tk,
        cx.view,
        "models_catalogue_url",
        &t("settings.models.catalogue_url"),
        &saved_only(&t("settings.models.catalogue_url.desc")),
        &stored,
        |v| {
            if is_http_url(v) {
                Ok(())
            } else {
                Err(t("settings.server_url.invalid"))
            }
        },
    ) {
        cx.settings.models_catalogue_url = url.trim().to_owned();
    }
    let options = labels(&[
        "settings.models.update.auto",
        "settings.models.update.manual",
    ]);
    let selected = usize::from(!cx.settings.models_catalogue_auto_update);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_models_update",
        &t("settings.models.update"),
        &saved_only(&t("settings.models.update.desc")),
        &options,
        selected,
    ) {
        cx.settings.models_catalogue_auto_update = i == 0;
    }
    unavailable_row(
        ui,
        tk,
        &t("settings.models.check_now"),
        &t("settings.models.check_now.desc"),
        "settings.reason.catalogue_update",
        None,
    );
    unavailable_row(
        ui,
        tk,
        &t("settings.models.unused"),
        &t("settings.models.unused.desc"),
        "settings.reason.model_manager",
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(data: DataLeavingView, locality: LocalityView, host: Option<&str>) -> PipelineRow {
        PipelineRow {
            role: PipelineRole::Translation,
            key: "zh>en".into(),
            provider: "p".into(),
            target: "t".into(),
            data,
            locality,
            host: host.map(str::to_owned),
            fallback: None,
        }
    }

    #[test]
    fn egress_lines_name_what_is_sent_where() {
        let local = egress_line(&row(
            DataLeavingView::Nothing,
            LocalityView::ThisComputer,
            None,
        ));
        assert_eq!(local, t("settings.pipeline.egress.local"));
        let cloud = egress_line(&row(
            DataLeavingView::Text,
            LocalityView::Remote,
            Some("api.x.com"),
        ));
        assert!(cloud.contains("api.x.com"), "{cloud}");
        let lan = egress_line(&row(
            DataLeavingView::Audio,
            LocalityView::LocalNetwork,
            Some("10.0.0.2"),
        ));
        assert!(lan.contains("10.0.0.2"), "{lan}");
        assert_ne!(cloud, lan);
    }

    #[test]
    fn a_role_label_names_role_and_languages() {
        let label = row_label(PipelineRole::Translation, "zh>en");
        assert!(label.contains('\u{2192}'), "{label}");
        assert_eq!(
            row_label(PipelineRole::Vad, ""),
            t("settings.pipeline.role.vad")
        );
    }

    #[test]
    fn an_unknown_problem_code_is_still_shown() {
        let p = PipelineProblem {
            subject: None,
            fallback: false,
            code: "zzz".into(),
            args: vec![],
        };
        assert!(problem_text(&p).contains("zzz"));
    }
}
