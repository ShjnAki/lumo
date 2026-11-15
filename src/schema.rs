//! Schéma de la fenêtre de réglages : quelles pages, quels champs, quels types et bornes.
//! Portable et testé : la couche Win32 (`winui.rs`) se contente de fabriquer des contrôles depuis ce schéma.

use serde_json::Value;

pub enum Kind {
    Bool,
    /// (valeur JSON, libellé affiché)
    Select(&'static [(&'static str, &'static str)]),
    Num { min: f64, max: f64, step: f64, unit: &'static str, pct: bool },
    /// "#RRGGBB" ou "#RRGGBBAA" (l'alpha existant est conservé).
    Color,
    Text,
}

pub struct Field {
    /// Chemin dans le JSON des réglages, ex. "ring.size".
    pub path: &'static str,
    pub label: &'static str,
    pub kind: Kind,
}

pub struct Page {
    pub title: &'static str,
    pub note: &'static str,
    pub fields: &'static [Field],
}

/// Nombre maximal de lignes par page (la fenêtre réserve cette hauteur).
pub const MAX_ROWS: usize = 8;

const SHAPES: &[(&str, &str)] = &[("circle", "Cercle"), ("h_bar", "Barre horizontale"), ("v_bar", "Barre verticale")];
const TRAILS: &[(&str, &str)] = &[
    ("line", "Ligne"),
    ("comet", "Comète"),
    ("glow", "Lueur"),
    ("dots", "Points"),
    ("stars", "Étoiles"),
    ("hearts", "Cœurs (kawaii)"),
    ("rainbow", "Arc-en-ciel"),
    ("skin", "Skin osu! (cursortrail.png)"),
];
const CLICKS: &[(&str, &str)] = &[
    ("ring", "Anneau"),
    ("ripple", "Onde (ripple)"),
    ("pulse", "Pulsation"),
    ("burst", "Rayons"),
    ("confetti", "Confettis"),
    ("stars", "Étoiles"),
    ("hearts", "Cœurs"),
];
const CURSORS: &[(&str, &str)] = &[
    ("off", "Curseur Windows (inchangé)"),
    ("system", "Image système (latence nulle)"),
    ("overlay", "Overlay (échelle au clic, rotation)"),
];

const fn num(min: f64, max: f64, step: f64, unit: &'static str) -> Kind {
    Kind::Num { min, max, step, unit, pct: false }
}
const fn pct() -> Kind {
    Kind::Num { min: 0.0, max: 1.0, step: 0.01, unit: "", pct: true }
}
const fn f(path: &'static str, label: &'static str, kind: Kind) -> Field {
    Field { path, label, kind }
}

pub static PAGES: &[Page] = &[
    Page {
        title: "Général",
        note: "Les changements s'appliquent immédiatement et sont enregistrés dans settings.json.",
        fields: &[
            f("enabled", "Effets activés", Kind::Bool),
            f("shake_to_locate", "Secouer la souris pour localiser le curseur", Kind::Bool),
        ],
    },
    Page {
        title: "Spotlight",
        note: "",
        fields: &[
            f("spotlight.enabled", "Activer le spotlight", Kind::Bool),
            f("spotlight.shape", "Forme", Kind::Select(SHAPES)),
            f("spotlight.size", "Taille", num(20.0, 1200.0, 5.0, " px")),
            f("spotlight.dim", "Assombrissement", pct()),
            f("spotlight.softness", "Douceur du bord", pct()),
        ],
    },
    Page {
        title: "Anneau",
        note: "",
        fields: &[
            f("ring.enabled", "Activer l'anneau", Kind::Bool),
            f("ring.size", "Taille", num(8.0, 200.0, 1.0, " px")),
            f("ring.thickness", "Épaisseur", num(1.0, 40.0, 0.5, " px")),
            f("ring.color", "Couleur", Kind::Color),
            f("ring.rgb", "Effet RGB (arc-en-ciel rotatif)", Kind::Bool),
            f("ring.rgb_speed", "Vitesse RGB", num(-720.0, 720.0, 10.0, " °/s")),
            f("ring.pulse", "Pulsation", Kind::Bool),
            f("ring.glow", "Halo lumineux (neon)", Kind::Bool),
        ],
    },
    Page {
        title: "Traînée",
        note: "",
        fields: &[
            f("trail.enabled", "Activer la traînée", Kind::Bool),
            f("trail.style", "Style", Kind::Select(TRAILS)),
            f("trail.color", "Couleur", Kind::Color),
            f("trail.size", "Taille", num(2.0, 80.0, 1.0, " px")),
            f("trail.length", "Durée", num(0.1, 3.0, 0.05, " s")),
        ],
    },
    Page {
        title: "Clics",
        note: "Fichier .wav : laisse vide pour le son intégré.",
        fields: &[
            f("click.enabled", "Activer les effets de clic", Kind::Bool),
            f("click.style", "Style", Kind::Select(CLICKS)),
            f("click.color", "Couleur (clic gauche)", Kind::Color),
            f("click.right_color", "Couleur (clic droit)", Kind::Color),
            f("click.size", "Taille", num(10.0, 400.0, 5.0, " px")),
            f("click.duration", "Durée", num(0.15, 3.0, 0.05, " s")),
            f("sound.enabled", "Son de clic", Kind::Bool),
            f("sound.file", "Fichier .wav", Kind::Text),
        ],
    },
    Page {
        title: "Annotations",
        note: "Touches de gauche : Ctrl, Alt, Shift, Win ; lettres, F1-F24 et CapsLock acceptés (ex. Ctrl+Alt).\r\nEffacement à 0 s : le tracé reste jusqu'à effacement manuel.",
        fields: &[
            f("annotate.enabled", "Activer les annotations", Kind::Bool),
            f("annotate.key", "Touches à maintenir", Kind::Text),
            f("annotate.color", "Couleur", Kind::Color),
            f("annotate.width", "Épaisseur", num(1.0, 60.0, 1.0, " px")),
            f("annotate.fade_delay", "Effacement après", num(0.0, 30.0, 0.5, " s")),
        ],
    },
    Page {
        title: "Curseur",
        note: "Skin osu! : dossier contenant cursor.png, cursormiddle.png, cursortrail.png. Vide = curseur généré.",
        fields: &[
            f("cursor.mode", "Mode", Kind::Select(CURSORS)),
            f("cursor.size", "Taille", num(16.0, 256.0, 2.0, " px")),
            f("cursor.color", "Couleur (curseur généré)", Kind::Color),
            f("cursor.outline", "Anneau blanc autour du curseur", Kind::Bool),
            f("cursor.press_scale", "Agrandissement au clic (overlay)", num(0.5, 2.5, 0.05, " x")),
            f("cursor.rotate", "Rotation (cursor.png, overlay)", Kind::Bool),
            f("cursor.skin_dir", "Dossier d'un skin osu!", Kind::Text),
        ],
    },
    Page {
        title: "Raccourcis",
        note: "Un modificateur (Ctrl, Alt, Shift, Win) est obligatoire, ex. Ctrl+Alt+L. Vide = aucun raccourci.",
        fields: &[
            f("hotkeys.toggle_all", "Tout activer/désactiver", Kind::Text),
            f("hotkeys.toggle_spotlight", "Spotlight", Kind::Text),
            f("hotkeys.toggle_ring", "Anneau", Kind::Text),
            f("hotkeys.toggle_trail", "Traînée", Kind::Text),
            f("hotkeys.toggle_clicks", "Effets de clic", Kind::Text),
            f("hotkeys.toggle_cursor", "Curseur custom", Kind::Text),
            f("hotkeys.toggle_annotate", "Annotations", Kind::Text),
            f("hotkeys.clear_annotations", "Effacer les annotations", Kind::Text),
        ],
    },
];

// ------------------------------------------------------------------ accès par chemin

pub fn get<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |o, k| o.get(k))
}

pub fn set(v: &mut Value, path: &str, new: Value) -> bool {
    let mut cur = v;
    let mut it = path.split('.').peekable();
    while let Some(k) = it.next() {
        if it.peek().is_none() {
            return match cur.as_object_mut() {
                Some(o) if o.contains_key(k) => {
                    o.insert(k.to_string(), new);
                    true
                }
                _ => false,
            };
        }
        match cur.get_mut(k) {
            Some(n) => cur = n,
            None => return false,
        }
    }
    false
}

// ------------------------------------------------------------------ conversions des contrôles

/// Nombre de crans d'un curseur (trackbar) : positions 0..=ticks.
pub fn ticks(min: f64, max: f64, step: f64) -> i32 {
    ((max - min) / step).round() as i32
}

pub fn pos_of(v: f64, min: f64, max: f64, step: f64) -> i32 {
    (((v - min) / step).round() as i32).clamp(0, ticks(min, max, step))
}

pub fn value_of(pos: i32, min: f64, step: f64) -> f64 {
    ((min + pos as f64 * step) * 10_000.0).round() / 10_000.0
}

pub fn format_num(v: f64, step: f64, unit: &str, pct: bool) -> String {
    if pct {
        return format!("{:.0} %", v * 100.0);
    }
    let decimals = if step >= 1.0 { 0 } else if step >= 0.1 { 1 } else { 2 };
    format!("{v:.decimals$}{unit}")
}

/// "#rrggbb[aa]" -> COLORREF Windows (0x00BBGGRR).
pub fn hex_to_colorref(hex: &str) -> u32 {
    let h = hex.trim_start_matches('#');
    let p = |i: usize| h.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or(0) as u32;
    p(0) | (p(2) << 8) | (p(4) << 16)
}

/// COLORREF -> "#rrggbb", en gardant l'alpha éventuel de l'ancienne valeur.
pub fn colorref_to_hex(c: u32, old: &str) -> String {
    let alpha = if old.len() == 9 { &old[7..] } else { "" };
    format!("#{:02x}{:02x}{:02x}{}", c & 0xFF, (c >> 8) & 0xFF, (c >> 16) & 0xFF, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn default_value() -> Value {
        serde_json::to_value(Settings::default()).unwrap()
    }

    fn reload(v: &Value) -> Settings {
        let mut s: Settings = serde_json::from_value(v.clone()).expect("le JSON doit rester désérialisable");
        s.sanitize();
        s
    }

    #[test]
    fn every_field_exists_with_the_right_type() {
        let d = default_value();
        for page in PAGES {
            assert!(page.fields.len() <= MAX_ROWS, "{} a trop de lignes", page.title);
            for fl in page.fields {
                let v = get(&d, fl.path).unwrap_or_else(|| panic!("chemin inconnu: {}", fl.path));
                match &fl.kind {
                    Kind::Bool => assert!(v.is_boolean(), "{}", fl.path),
                    Kind::Num { .. } => assert!(v.is_number(), "{}", fl.path),
                    Kind::Select(_) | Kind::Color | Kind::Text => assert!(v.is_string(), "{}", fl.path),
                }
            }
        }
    }

    #[test]
    fn every_setting_has_a_field() {
        // aucun réglage (hors hotkeys listés) ne doit manquer dans l'interface
        fn leaves(prefix: &str, v: &Value, out: &mut Vec<String>) {
            match v {
                Value::Object(o) => o.iter().for_each(|(k, x)| leaves(&if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") }, x, out)),
                _ => out.push(prefix.to_string()),
            }
        }
        let mut all = Vec::new();
        leaves("", &default_value(), &mut all);
        let known: Vec<&str> = PAGES.iter().flat_map(|p| p.fields.iter().map(|f| f.path)).collect();
        let missing: Vec<_> = all.iter().filter(|p| !known.contains(&p.as_str())).collect();
        assert!(missing.is_empty(), "réglages sans champ dans l'interface: {missing:?}");
    }

    #[test]
    fn select_values_are_valid_variants() {
        for page in PAGES {
            for fl in page.fields {
                if let Kind::Select(opts) = &fl.kind {
                    for (val, _) in *opts {
                        let mut v = default_value();
                        assert!(set(&mut v, fl.path, Value::from(*val)));
                        reload(&v); // panique si la variante n'existe pas côté serde
                    }
                }
            }
        }
    }

    #[test]
    fn numeric_bounds_survive_sanitize() {
        for page in PAGES {
            for fl in page.fields {
                if let Kind::Num { min, max, step, .. } = &fl.kind {
                    for target in [*min, *max] {
                        let mut v = default_value();
                        set(&mut v, fl.path, Value::from(target));
                        let back = serde_json::to_value(reload(&v)).unwrap();
                        let got = get(&back, fl.path).unwrap().as_f64().unwrap();
                        assert!((got - target).abs() < 1e-4, "{}: {target} devient {got} après sanitize", fl.path);
                    }
                    let t = ticks(*min, *max, *step);
                    assert_eq!(value_of(0, *min, *step), *min);
                    assert!((value_of(t, *min, *step) - *max).abs() < 1e-3, "{}: max inatteignable avec le pas", fl.path);
                }
            }
        }
    }

    #[test]
    fn default_values_fit_the_sliders() {
        let d = default_value();
        for page in PAGES {
            for fl in page.fields {
                if let Kind::Num { min, max, step, .. } = &fl.kind {
                    let v = get(&d, fl.path).unwrap().as_f64().unwrap();
                    assert!(v >= *min && v <= *max, "{} hors curseur", fl.path);
                    let back = value_of(pos_of(v, *min, *max, *step), *min, *step);
                    assert!((back - v).abs() < *step / 2.0 + 1e-6, "{}: {v} -> {back}", fl.path);
                }
            }
        }
    }

    #[test]
    fn colors_roundtrip_keep_alpha() {
        assert_eq!(hex_to_colorref("#ff8000"), 0x0000_80ff);
        assert_eq!(colorref_to_hex(0x0000_80ff, "#000000"), "#ff8000");
        assert_eq!(colorref_to_hex(0x0000_80ff, "#ffc400dc"), "#ff8000dc");
        assert_eq!(hex_to_colorref("#ffc400dc"), hex_to_colorref("#ffc400"));
    }

    #[test]
    fn set_rejects_unknown_paths() {
        let mut v = default_value();
        assert!(!set(&mut v, "ring.nope", Value::from(1)));
        assert!(!set(&mut v, "nope.size", Value::from(1)));
        assert!(set(&mut v, "ring.size", Value::from(42.0)));
        assert_eq!(get(&v, "ring.size").unwrap().as_f64(), Some(42.0));
    }

    #[test]
    fn number_formatting() {
        assert_eq!(format_num(0.65, 0.01, "", true), "65 %");
        assert_eq!(format_num(30.0, 1.0, " px", false), "30 px");
        assert_eq!(format_num(0.45, 0.05, " s", false), "0.45 s");
        assert_eq!(format_num(1.25, 0.05, " x", false), "1.25 x");
    }
}
