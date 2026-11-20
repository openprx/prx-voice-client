//! Bundled fonts from the design standard (section "Typography").
//!
//! - UI text: Hanken Grotesk at weights 400, 500 and 600.
//! - Monospace: JetBrains Mono at weights 400 and 600.
//! - Chinese: Noto Sans SC at weights 400 and 600 (subset, see
//!   `scripts/build_fonts.py` for the exact coverage).
//!
//! egui has no per-text weight, so each weight is a separate font family:
//! the default `Proportional` and `Monospace` families are weight 400, and
//! [`medium`], [`semibold`] and [`mono_semibold`] are named families.
//!
//! Fallback order in every family: the Latin face, the embedded Chinese
//! face, the system CJK face (added with [`add_system_cjk`]), then egui's
//! built-in fonts for symbols and emoji.
//!
//! Fonts are SIL OFL 1.1; see `assets/fonts/OFL-*.txt` and the repository's
//! `THIRD_PARTY_NOTICES.md`.

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily, Id};

const HANKEN_REGULAR: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Regular.ttf");
const HANKEN_MEDIUM: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-Medium.ttf");
const HANKEN_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/HankenGrotesk-SemiBold.ttf");
const JBM_REGULAR: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
const JBM_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-SemiBold.ttf");
const NOTO_REGULAR: &[u8] = include_bytes!("../assets/fonts/NotoSansSC-Regular.ttf");
const NOTO_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/NotoSansSC-SemiBold.ttf");

/// Font data keys.
const K_HANKEN_400: &str = "HankenGrotesk-Regular";
const K_HANKEN_500: &str = "HankenGrotesk-Medium";
const K_HANKEN_600: &str = "HankenGrotesk-SemiBold";
const K_JBM_400: &str = "JetBrainsMono-Regular";
const K_JBM_600: &str = "JetBrainsMono-SemiBold";
const K_NOTO_400: &str = "NotoSansSC-Regular";
const K_NOTO_600: &str = "NotoSansSC-SemiBold";
/// Key of the system CJK fallback font data.
const K_SYSTEM_CJK: &str = "system-cjk";

/// Name of the weight-500 UI family.
pub const SANS_MEDIUM: &str = "rf-sans-medium";
/// Name of the weight-600 UI family.
pub const SANS_SEMIBOLD: &str = "rf-sans-semibold";
/// Name of the weight-600 monospace family.
pub const MONO_SEMIBOLD: &str = "rf-mono-semibold";

/// UI text, weight 500 (button labels, row titles).
pub fn medium() -> FontFamily {
    FontFamily::Name(SANS_MEDIUM.into())
}

/// UI text, weight 600 (headings, emphasis).
pub fn semibold() -> FontFamily {
    FontFamily::Name(SANS_SEMIBOLD.into())
}

/// Monospace text, weight 600 (pairing codes).
pub fn mono_semibold() -> FontFamily {
    FontFamily::Name(MONO_SEMIBOLD.into())
}

/// Every family this module defines, in a stable order.
pub fn families() -> Vec<FontFamily> {
    vec![
        FontFamily::Proportional,
        medium(),
        semibold(),
        FontFamily::Monospace,
        mono_semibold(),
    ]
}

/// Embedded fonts and the families built on them. The system CJK font is not
/// included; add it with [`add_system_cjk`].
pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    for (key, bytes) in [
        (K_HANKEN_400, HANKEN_REGULAR),
        (K_HANKEN_500, HANKEN_MEDIUM),
        (K_HANKEN_600, HANKEN_SEMIBOLD),
        (K_JBM_400, JBM_REGULAR),
        (K_JBM_600, JBM_SEMIBOLD),
        (K_NOTO_400, NOTO_REGULAR),
        (K_NOTO_600, NOTO_SEMIBOLD),
    ] {
        defs.font_data
            .insert(key.to_owned(), Arc::new(FontData::from_static(bytes)));
    }

    // egui's built-in fonts stay at the end of every stack (symbols, emoji).
    let builtin_sans = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let builtin_mono = defs
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    let stack = |latin: &str, zh: &str, builtin: &[String]| -> Vec<String> {
        let mut v = vec![latin.to_owned(), zh.to_owned()];
        v.extend(builtin.iter().cloned());
        v
    };
    defs.families.insert(
        FontFamily::Proportional,
        stack(K_HANKEN_400, K_NOTO_400, &builtin_sans),
    );
    // There is no embedded weight-500 Chinese face: weight 500 uses regular.
    defs.families
        .insert(medium(), stack(K_HANKEN_500, K_NOTO_400, &builtin_sans));
    defs.families
        .insert(semibold(), stack(K_HANKEN_600, K_NOTO_600, &builtin_sans));
    defs.families.insert(
        FontFamily::Monospace,
        stack(K_JBM_400, K_NOTO_400, &builtin_mono),
    );
    defs.families
        .insert(mono_semibold(), stack(K_JBM_600, K_NOTO_600, &builtin_mono));
    defs
}

/// Insert a system CJK font right after the embedded Chinese face in every
/// family, so it is used only for characters the embedded subset lacks and
/// still comes before egui's built-in symbol and emoji fonts.
pub fn add_system_cjk(defs: &mut FontDefinitions, data: Vec<u8>) {
    defs.font_data.insert(
        K_SYSTEM_CJK.to_owned(),
        Arc::new(FontData::from_owned(data)),
    );
    for family in families() {
        if let Some(stack) = defs.families.get_mut(&family) {
            let at = stack
                .iter()
                .position(|k| k == K_NOTO_400 || k == K_NOTO_600)
                .map_or(stack.len(), |i| i + 1);
            stack.insert(at, K_SYSTEM_CJK.to_owned());
        }
    }
}

/// Marker stored in the context once fonts were installed through this
/// module.
#[derive(Clone, Copy)]
struct Installed;

fn marker() -> Id {
    Id::new("prxvc-desktop-ui-fonts-installed")
}

/// Install the embedded fonts (and the system CJK font, if given) into `ctx`.
/// Takes effect at the start of the next frame.
pub fn install(ctx: &Context, system_cjk: Option<Vec<u8>>) {
    let mut defs = definitions();
    if let Some(data) = system_cjk {
        add_system_cjk(&mut defs, data);
    }
    ctx.set_fonts(defs);
    ctx.data_mut(|d| d.insert_temp(marker(), Installed));
}

/// Install the embedded fonts unless [`install`] already ran for `ctx`. Used
/// by the theme so that a context that never had its fonts set still has
/// every family the text styles refer to.
pub fn ensure_installed(ctx: &Context) {
    let done = ctx.data(|d| d.get_temp::<Installed>(marker()).is_some());
    if !done {
        install(ctx, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::FontId;

    fn ctx_with(system: Option<Vec<u8>>) -> Context {
        let ctx = Context::default();
        install(&ctx, system);
        // Fonts become active at the start of the next pass.
        let _ = ctx.run(Default::default(), |_| {});
        ctx
    }

    #[test]
    fn every_embedded_font_parses_and_every_family_resolves() {
        let ctx = ctx_with(None);
        for family in families() {
            let id = FontId::new(14.0, family.clone());
            let width = ctx.fonts(|f| f.glyph_width(&id, 'A'));
            assert!(width > 0.0, "family {family:?} has no usable Latin glyph");
            let height = ctx.fonts(|f| f.row_height(&id));
            assert!(height > 0.0, "family {family:?} has no row height");
        }
    }

    #[test]
    fn stacks_are_ordered_latin_then_chinese_then_system_then_builtin() {
        let mut defs = definitions();
        add_system_cjk(&mut defs, NOTO_REGULAR.to_vec());
        let expect_first = [
            (FontFamily::Proportional, K_HANKEN_400, K_NOTO_400),
            (medium(), K_HANKEN_500, K_NOTO_400),
            (semibold(), K_HANKEN_600, K_NOTO_600),
            (FontFamily::Monospace, K_JBM_400, K_NOTO_400),
            (mono_semibold(), K_JBM_600, K_NOTO_600),
        ];
        for (family, latin, zh) in expect_first {
            let stack = &defs.families[&family];
            assert_eq!(stack[0], latin, "{family:?}");
            assert_eq!(stack[1], zh, "{family:?}");
            assert_eq!(stack[2], K_SYSTEM_CJK, "{family:?}");
            assert!(
                stack.iter().any(|k| k == "NotoEmoji-Regular"),
                "{family:?} keeps egui's emoji font"
            );
            assert!(stack.len() > 3, "{family:?}");
            for key in stack {
                assert!(defs.font_data.contains_key(key), "{key} has no font data");
            }
        }
    }

    #[test]
    fn embedded_files_are_distinct_static_fonts() {
        for (name, bytes) in [
            (K_HANKEN_400, HANKEN_REGULAR),
            (K_HANKEN_500, HANKEN_MEDIUM),
            (K_HANKEN_600, HANKEN_SEMIBOLD),
            (K_JBM_400, JBM_REGULAR),
            (K_JBM_600, JBM_SEMIBOLD),
            (K_NOTO_400, NOTO_REGULAR),
            (K_NOTO_600, NOTO_SEMIBOLD),
        ] {
            // TrueType scaler tag 0x00010000.
            assert_eq!(&bytes[..4], &[0, 1, 0, 0], "{name} is not a TrueType font");
        }
        assert_ne!(HANKEN_REGULAR, HANKEN_MEDIUM);
        assert_ne!(HANKEN_MEDIUM, HANKEN_SEMIBOLD);
    }

    #[test]
    fn ensure_installed_does_not_override_install() {
        let ctx = Context::default();
        install(&ctx, Some(NOTO_REGULAR.to_vec()));
        ensure_installed(&ctx);
        let _ = ctx.run(Default::default(), |_| {});
        let id = FontId::new(14.0, FontFamily::Proportional);
        assert!(ctx.fonts(|f| f.has_glyph(&id, '汉')));
    }
}
