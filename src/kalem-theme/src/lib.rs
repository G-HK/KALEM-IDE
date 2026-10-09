//! Themes for the IDE.
//!
//! A theme is a TOML file with a `[light]` and a `[dark]` palette. The built-in theme "Kalem" is
//! the two basic palettes (light = Black Panda palette). Users add their own theme files to the
//! `themes` folder in their config folder; anything a theme leaves out falls back to "Kalem".
//! This crate has no UI toolkit in it: the app maps the tokens to its own colour types.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const BUILTIN_TOML: &str = include_str!("../../../themes/kalem.toml");

/// Every colour token a palette can set.
pub const TOKENS: &[&str] = &[
    "background",
    "panel",
    "foreground",
    "muted_foreground",
    "border",
    "list_active",
    "accent",
    "success",
    "warning",
    "danger",
    "folder",
    "project",
    "solution",
    "lock",
    // Editor tab colours, one per project (cycled).
    "tab1",
    "tab2",
    "tab3",
    "tab4",
    "tab5",
    "tab6",
    "tab7",
    "tab8",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
    /// Follow the operating system.
    System,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Dark => "dark",
            Mode::System => "system",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_lowercase().as_str() {
            "light" => Some(Mode::Light),
            "dark" => Some(Mode::Dark),
            "system" => Some(Mode::System),
            _ => None,
        }
    }
}

/// One palette: colour tokens plus per-file-type badge colours, as 0xRRGGBB.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Palette {
    pub colors: BTreeMap<String, u32>,
    pub badges: BTreeMap<String, u32>,
}

impl Palette {
    /// Colour of a token. Built-in palettes define all tokens, so this never misses for them.
    pub fn get(&self, token: &str) -> u32 {
        self.colors.get(token).copied().unwrap_or(0xff00ff)
    }

    /// Badge colour override for a file badge label (case-insensitive).
    pub fn badge(&self, label: &str) -> Option<u32> {
        self.badges.get(&label.to_lowercase()).copied()
    }

    fn overlay(&mut self, over: &Palette) {
        self.colors
            .extend(over.colors.iter().map(|(k, v)| (k.clone(), *v)));
        self.badges
            .extend(over.badges.iter().map(|(k, v)| (k.clone(), *v)));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub light: Palette,
    pub dark: Palette,
    /// Problems found while reading the file (unknown token, bad colour). The theme still loads.
    pub warnings: Vec<String>,
}

impl Theme {
    pub fn palette(&self, dark: bool) -> &Palette {
        if dark {
            &self.dark
        } else {
            &self.light
        }
    }

    /// The built-in theme "Kalem".
    pub fn builtin() -> Theme {
        Theme::from_toml(BUILTIN_TOML, None).expect("built-in theme is valid")
    }

    /// Parse a theme file. Colours not set fall back to `base` (default: the built-in theme).
    pub fn from_toml(text: &str, base: Option<&Theme>) -> Result<Theme, String> {
        let doc: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let name = doc
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("theme file needs a `name = \"...\"` line")?
            .to_string();
        let builtin;
        let base = match base {
            Some(b) => Some(b),
            None if name == "Kalem" && text == BUILTIN_TOML => None,
            None => {
                builtin = Theme::builtin();
                Some(&builtin)
            }
        };
        let mut warnings = Vec::new();
        let mut light = base.map(|b| b.light.clone()).unwrap_or_default();
        let mut dark = base.map(|b| b.dark.clone()).unwrap_or_default();
        for (key, target) in [("light", &mut light), ("dark", &mut dark)] {
            if let Some(t) = doc.get(key).and_then(|v| v.as_table()) {
                let over = read_palette(key, t, &mut warnings);
                target.overlay(&over);
            }
        }
        for k in doc.keys() {
            if !matches!(k.as_str(), "name" | "light" | "dark") {
                warnings.push(format!("unknown top-level key `{k}`"));
            }
        }
        Ok(Theme {
            name,
            light,
            dark,
            warnings,
        })
    }
}

fn read_palette(section: &str, t: &toml::Table, warnings: &mut Vec<String>) -> Palette {
    let mut p = Palette::default();
    for (k, v) in t {
        if k == "badges" {
            if let Some(b) = v.as_table() {
                for (label, c) in b {
                    match c.as_str().and_then(parse_hex) {
                        Some(c) => {
                            p.badges.insert(label.to_lowercase(), c);
                        }
                        None => warnings
                            .push(format!("[{section}.badges] {label}: not a #RRGGBB colour")),
                    }
                }
            }
            continue;
        }
        if !TOKENS.contains(&k.as_str()) {
            warnings.push(format!("[{section}] unknown colour `{k}`"));
            continue;
        }
        match v.as_str().and_then(parse_hex) {
            Some(c) => {
                p.colors.insert(k.clone(), c);
            }
            None => warnings.push(format!("[{section}] {k}: not a #RRGGBB colour")),
        }
    }
    p
}

/// `#RRGGBB` (or `RRGGBB`) to 0xRRGGBB.
pub fn parse_hex(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

/// The user's config folder for Kalem (`KALEM_CONFIG_DIR` wins).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("KALEM_CONFIG_DIR") {
        return Some(PathBuf::from(p));
    }
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("kalem"))
}

/// Built-in theme first, then every `*.toml` in `dir`, sorted by file name. Bad files are skipped and
/// reported in the second list as `file: problem`.
pub fn load_all(dir: Option<&Path>) -> (Vec<Theme>, Vec<String>) {
    let mut themes = vec![Theme::builtin()];
    let mut errors = Vec::new();
    let Some(dir) = dir else {
        return (themes, errors);
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (themes, errors);
    };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    files.sort();
    for f in files {
        let fname = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match std::fs::read_to_string(&f)
            .map_err(|e| e.to_string())
            .and_then(|t| Theme::from_toml(&t, None))
        {
            Ok(t) if themes.iter().any(|x| x.name == t.name) => errors.push(format!(
                "{fname}: a theme named \"{}\" already exists",
                t.name
            )),
            Ok(t) => {
                errors.extend(t.warnings.iter().map(|w| format!("{fname}: {w}")));
                themes.push(t);
            }
            Err(e) => errors.push(format!("{fname}: {e}")),
        }
    }
    (themes, errors)
}

/// What the user picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub theme: String,
    pub mode: Mode,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "Kalem".into(),
            mode: Mode::Dark,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        if let Ok(t) = text.parse::<toml::Table>() {
            if let Some(n) = t.get("theme").and_then(|v| v.as_str()) {
                s.theme = n.to_string();
            }
            if let Some(m) = t.get("mode").and_then(|v| v.as_str()).and_then(Mode::parse) {
                s.mode = m;
            }
        }
        s
    }

    pub fn to_toml(&self) -> String {
        let mut t = toml::Table::new();
        t.insert("theme".into(), toml::Value::String(self.theme.clone()));
        t.insert(
            "mode".into(),
            toml::Value::String(self.mode.as_str().into()),
        );
        toml::to_string(&t).unwrap_or_default()
    }

    pub fn load(dir: Option<&Path>) -> Settings {
        dir.and_then(|d| std::fs::read_to_string(d.join("settings.toml")).ok())
            .map(|t| Settings::parse(&t))
            .unwrap_or_default()
    }

    pub fn save(&self, dir: Option<&Path>) -> Result<(), String> {
        let d = dir.ok_or("no config folder")?;
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        std::fs::write(d.join("settings.toml"), self.to_toml()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_defines_every_token_in_both_palettes() {
        let t = Theme::builtin();
        assert!(t.warnings.is_empty(), "{:?}", t.warnings);
        for tok in TOKENS {
            assert!(t.light.colors.contains_key(*tok), "light missing {tok}");
            assert!(t.dark.colors.contains_key(*tok), "dark missing {tok}");
        }
        assert_eq!(t.dark.badge("YML"), Some(0xe0524d));
    }

    #[test]
    fn user_theme_overrides_only_what_it_sets() {
        let t = Theme::from_toml(
            "name = \"Mine\"\n[dark]\naccent = \"#ff8800\"\n[dark.badges]\nrs = \"#00ff00\"\n",
            None,
        )
        .unwrap();
        let b = Theme::builtin();
        assert_eq!(t.dark.get("accent"), 0xff8800);
        assert_eq!(t.dark.badge("rs"), Some(0x00ff00));
        assert_eq!(t.dark.get("background"), b.dark.get("background"));
        assert_eq!(t.light, b.light);
    }

    #[test]
    fn bad_values_become_warnings() {
        let t = Theme::from_toml(
            "name = \"X\"\nfoo = 1\n[light]\naccent = \"red\"\nnope = \"#000000\"\n",
            None,
        )
        .unwrap();
        assert_eq!(t.warnings.len(), 3, "{:?}", t.warnings);
        assert_eq!(t.light.get("accent"), Theme::builtin().light.get("accent"));
    }

    #[test]
    fn name_is_required() {
        assert!(Theme::from_toml("[dark]\naccent = \"#000000\"\n", None).is_err());
    }

    #[test]
    fn hex() {
        assert_eq!(parse_hex("#1e1e1e"), Some(0x1e1e1e));
        assert_eq!(parse_hex("ABCDEF"), Some(0xabcdef));
        assert_eq!(parse_hex("#fff"), None);
        assert_eq!(parse_hex("#gggggg"), None);
    }

    #[test]
    fn settings_round_trip_and_load_all() {
        let d = std::env::temp_dir().join(format!("kalem-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("themes")).unwrap();
        std::fs::write(
            d.join("themes/a.toml"),
            "name = \"Ocean\"\n[dark]\naccent = \"#0099ff\"\n",
        )
        .unwrap();
        std::fs::write(d.join("themes/bad.toml"), "not toml [[[").unwrap();
        std::fs::write(d.join("themes/dup.toml"), "name = \"Kalem\"\n").unwrap();
        let (themes, errors) = load_all(Some(&d.join("themes")));
        assert_eq!(
            themes.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["Kalem", "Ocean"]
        );
        assert_eq!(errors.len(), 2, "{errors:?}");
        let s = Settings {
            theme: "Ocean".into(),
            mode: Mode::Light,
        };
        s.save(Some(&d)).unwrap();
        assert_eq!(Settings::load(Some(&d)), s);
        assert_eq!(Settings::load(None), Settings::default());
        let _ = std::fs::remove_dir_all(&d);
    }
}
