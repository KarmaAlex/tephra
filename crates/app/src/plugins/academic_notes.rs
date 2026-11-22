//! Academic Notes (replaces the community plugin of the same name). M2: callout styling for its
//! environments (D7) with the vault's palettes. M3 adds numbering, references and algorithm
//! pseudocode.

use gpui_kit::*;
use serde_json::{Map, Value};

use super::api::*;

/// Environment groups and their aliases, as in the plugin's stylesheet.
const ENVS: &[(&str, &str, &[&str])] = &[
    ("def", "Definition", &["def", "definition", "asm", "assumption"]),
    ("thm", "Theorem", &["thm", "theorem", "axiom", "axm", "hyp", "hypothesis"]),
    ("lem", "Lemma", &["lem", "lemma"]),
    ("prop", "Proposition", &["prop", "prp", "proposition"]),
    ("cor", "Corollary", &["cor", "corollary"]),
    ("claim", "Claim", &["claim", "clm", "conjecture", "cnj"]),
    ("example", "Example", &["example", "exm", "exercise", "exr", "ex", "exa"]),
    ("solution", "Solution", &["solution", "sol"]),
    ("proof", "Proof", &["proof", "pf"]),
    ("remark", "Remark", &["remark", "rem", "rmk"]),
    ("algorithm", "Algorithm", &["algorithm", "alg"]),
];

/// Display names of aliases that differ from their group's (e.g. `axiom` -> Axiom).
fn alias_label(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "asm" | "assumption" => "Assumption",
        "axiom" | "axm" => "Axiom",
        "hyp" | "hypothesis" => "Hypothesis",
        "conjecture" | "cnj" => "Conjecture",
        "exercise" | "exr" => "Exercise",
        _ => return None,
    })
}

/// (def, thm, lem, prop, cor, claim, example, proof) accents per palette.
fn palette(name: &str) -> [u32; 8] {
    match name {
        "colorful-dark" => [0x86ca98, 0x83bbce, 0xb7a7ca, 0xb8c984, 0x8dcbb4, 0x9baed0, 0xdec178, 0xa8b898],
        "colorful" => [0x247651, 0x286b76, 0x71628c, 0x62752e, 0x42786b, 0x44698c, 0x946b2f, 0x687252],
        _ /* forest */ => [0x2e7055, 0x296a70, 0x537354, 0x73723d, 0x367d69, 0x477486, 0x87653e, 0x5c6d61],
    }
}

struct Styles {
    light: [u32; 8],
    dark: [u32; 8],
}

impl CalloutRenderer for Styles {
    fn style(&self, kind: &str, _meta: &str, dark: bool) -> Option<CalloutStyle> {
        let (group, label, _) = ENVS.iter().find(|(_, _, aliases)| aliases.contains(&kind))?;
        let p = if dark { &self.dark } else { &self.light };
        let ix = match *group {
            "def" | "algorithm" => 0,
            "thm" => 1,
            "lem" => 2,
            "prop" => 3,
            "cor" => 4,
            "claim" => 5,
            "example" => 6,
            _ => 7, // solution, proof, remark
        };
        let look = match *group {
            "proof" | "remark" => CalloutLook::Soft,
            "algorithm" => CalloutLook::Rules,
            _ => CalloutLook::Badge,
        };
        let label = alias_label(kind).unwrap_or(label);
        Some(CalloutStyle { color: rgb(p[ix]).into(), look, icon: None, label: Some(label.to_string()) })
    }
}

pub struct AcademicNotes {
    manifest: PluginManifest,
}

impl AcademicNotes {
    pub fn new() -> Self {
        let palettes = vec![("forest", "Forest"), ("colorful", "Colorful"), ("colorful-dark", "Colorful (dark)")];
        Self {
            manifest: PluginManifest {
                id: "academic-notes",
                name: "Academic Notes",
                version: "0.1.0",
                description: "Theorem-like environments (definition, theorem, proof, …) as styled callouts. Numbering and references arrive in M3.",
                enabled_by_default: true,
                settings: vec![
                    SettingSpec { key: "lightPalette", label: "Light palette", description: "Accent colours in the light theme.", kind: SettingKind::Choice(palettes.clone()), default: Value::from("forest") },
                    SettingSpec { key: "darkPalette", label: "Dark palette", description: "Accent colours in the dark theme.", kind: SettingKind::Choice(palettes), default: Value::from("colorful-dark") },
                ],
                code_languages: &[],
            },
        }
    }
}

impl Plugin for AcademicNotes {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    fn on_load(&mut self, settings: &Map<String, Value>, reg: &mut Registrar) {
        let get = |k: &str, d: &str| settings.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
        reg.callout_renderer(Styles { light: palette(&get("lightPalette", "forest")), dark: palette(&get("darkPalette", "colorful-dark")) });
    }
}

#[cfg(test)]
mod tests {
    use super::{palette, CalloutLook, CalloutRenderer, Styles};
    use gpui_kit::rgb;

    #[test]
    fn claims_its_environments_only() {
        let s = Styles { light: palette("forest"), dark: palette("colorful-dark") };
        let thm = s.style("thm", "", false).unwrap();
        assert_eq!((thm.look, thm.label.as_deref()), (CalloutLook::Badge, Some("Theorem")));
        assert_eq!(thm.color, rgb(0x296a70).into());
        assert_eq!(s.style("thm", "", true).unwrap().color, rgb(0x83bbce).into());
        assert_eq!(s.style("pf", "", false).unwrap().look, CalloutLook::Soft);
        assert_eq!(s.style("axiom", "", false).unwrap().label.as_deref(), Some("Axiom"));
        assert_eq!(s.style("algorithm", "lines=true", false).unwrap().look, CalloutLook::Rules);
        assert!(s.style("note", "", false).is_none(), "Obsidian's own types stay with the core");
        assert!(s.style("warning", "", false).is_none());
    }
}
