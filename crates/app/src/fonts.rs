//! Embedded fonts, so the app renders identically on every machine.
//!
//! GPUI on Linux maps the theme's default sans family `.SystemUIFont` to the hardcoded
//! family name "IBM Plex Sans" (`platform/linux/text_system.rs`), and gpui-component's
//! default mono is "DejaVu Sans Mono" (probed against installed fonts at startup). Neither
//! is guaranteed on a target machine — e.g. an Arch laptop without `ttf-ibm-plex` /
//! `ttf-dejavu` silently falls back to whatever font is installed, so text weighting
//! (400/500/600/700) looks different there than on the dev machine. The font files in
//! `assets/fonts/` (OFL / Bitstream Vera, see the licenses there) are embedded into the
//! binary and the theme families are set to them explicitly, so every machine resolves the
//! same faces. The files are unmodified copies of the system packages, so rendering on a
//! machine that has them installed is unchanged.

use std::borrow::Cow;

use gpui_kit::*;

/// IBM Plex Sans — UI and editor text (sans). All weights the app uses: 400/500/600/700
/// plus italics.
const SANS: &[Cow<'static, [u8]>] = &[
    Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-Italic.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-MediumItalic.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-SemiBoldItalic.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-Bold.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-BoldItalic.ttf").as_slice()),
];

/// DejaVu Sans Mono — code blocks and other monospace text (gpui-component's Linux default).
const MONO: &[Cow<'static, [u8]>] = &[
    Cow::Borrowed(include_bytes!("../assets/fonts/DejaVuSansMono.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/DejaVuSansMono-Bold.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/DejaVuSansMono-Oblique.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/DejaVuSansMono-BoldOblique.ttf").as_slice()),
];

/// DejaVu Sans — glyph fallback for symbols missing from the families above (GPUI's
/// fallback stack ends in "Noto Sans" / "DejaVu Sans" on Linux).
const FALLBACK: &[Cow<'static, [u8]>] = &[
    Cow::Borrowed(include_bytes!("../assets/fonts/DejaVuSans.ttf").as_slice()),
    Cow::Borrowed(include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf").as_slice()),
];

/// The family names the theme is set to, after the embedded fonts are registered.
pub const SANS_FAMILY: &str = "IBM Plex Sans";
pub const MONO_FAMILY: &str = "DejaVu Sans Mono";

/// Registers the embedded fonts and points the theme at them. Call once, after
/// `gpui_kit::init` (whose mono-font probe only replaces the *default* family, so an
/// explicit one set here is left alone).
pub fn load(cx: &mut App) {
    let mut fonts = Vec::new();
    fonts.extend_from_slice(SANS);
    fonts.extend_from_slice(MONO);
    fonts.extend_from_slice(FALLBACK);
    cx.text_system().add_fonts(fonts).expect("embedded fonts are valid");
    let theme = cx.global_mut::<gpui_kit::component::theme::Theme>();
    theme.font_family = SANS_FAMILY.into();
    theme.mono_font_family = MONO_FAMILY.into();
}
