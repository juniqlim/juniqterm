use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use growterm_macos::key_convert::char_to_keycode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyModeAction {
    Down,
    Up,
    Visual,
    HalfPageDown,
    HalfPageUp,
    Yank,
    Exit,
}

fn deserialize_keys<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => Ok(vec![s]),
        OneOrMany::Many(v) => Ok(v),
    }
}

fn default_down() -> Vec<String> { vec!["j".into()] }
fn default_up() -> Vec<String> { vec!["k".into()] }
fn default_visual() -> Vec<String> { vec!["v".into()] }
fn default_half_page_down() -> Vec<String> { vec!["h".into(), "d".into()] }
fn default_half_page_up() -> Vec<String> { vec!["l".into(), "u".into()] }
fn default_yank() -> Vec<String> { vec!["y".into()] }
fn default_exit() -> Vec<String> { vec!["q".into(), "Escape".into(), "`".into()] }

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CopyModeKeys {
    #[serde(default = "default_down", deserialize_with = "deserialize_keys")]
    pub down: Vec<String>,
    #[serde(default = "default_up", deserialize_with = "deserialize_keys")]
    pub up: Vec<String>,
    #[serde(default = "default_visual", deserialize_with = "deserialize_keys")]
    pub visual: Vec<String>,
    #[serde(default = "default_half_page_down", deserialize_with = "deserialize_keys")]
    pub half_page_down: Vec<String>,
    #[serde(default = "default_half_page_up", deserialize_with = "deserialize_keys")]
    pub half_page_up: Vec<String>,
    #[serde(default = "default_yank", deserialize_with = "deserialize_keys")]
    pub yank: Vec<String>,
    #[serde(default = "default_exit", deserialize_with = "deserialize_keys")]
    pub exit: Vec<String>,
}

impl Default for CopyModeKeys {
    fn default() -> Self {
        Self {
            down: default_down(),
            up: default_up(),
            visual: default_visual(),
            half_page_down: default_half_page_down(),
            half_page_up: default_half_page_up(),
            yank: default_yank(),
            exit: default_exit(),
        }
    }
}

impl CopyModeKeys {
    pub fn build_action_map(&self) -> HashMap<u16, CopyModeAction> {
        let mut map = HashMap::new();
        let bindings: &[(&[String], CopyModeAction)] = &[
            (&self.down, CopyModeAction::Down),
            (&self.up, CopyModeAction::Up),
            (&self.visual, CopyModeAction::Visual),
            (&self.half_page_down, CopyModeAction::HalfPageDown),
            (&self.half_page_up, CopyModeAction::HalfPageUp),
            (&self.yank, CopyModeAction::Yank),
            (&self.exit, CopyModeAction::Exit),
        ];
        for (keys, action) in bindings {
            for key_str in *keys {
                if let Some(kc) = char_to_keycode(key_str) {
                    map.insert(kc, *action);
                }
            }
        }
        map
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub pomodoro: bool,
    #[serde(default)]
    pub response_timer: bool,
    #[serde(default = "default_true")]
    pub coaching: bool,
    #[serde(default)]
    pub transparent_tab_bar: bool,
    #[serde(default = "default_header_opacity")]
    pub header_opacity: f32,
    #[serde(default)]
    pub coaching_command: Option<String>,
    #[serde(default)]
    pub copy_mode_keys: CopyModeKeys,
    #[serde(default)]
    pub window_width: Option<f64>,
    #[serde(default)]
    pub window_height: Option<f64>,
    #[serde(default)]
    pub window_x: Option<f64>,
    #[serde(default)]
    pub window_y: Option<f64>,
}

fn default_font_family() -> String {
    "FiraCodeNerdFontMono-Retina".to_string()
}

fn default_font_size() -> f32 {
    32.0
}

fn default_header_opacity() -> f32 {
    0.8
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            pomodoro: false,
            response_timer: false,
            coaching: true,
            transparent_tab_bar: false,
            header_opacity: default_header_opacity(),
            coaching_command: None,
            copy_mode_keys: CopyModeKeys::default(),
            window_width: None,
            window_height: None,
            window_x: None,
            window_y: None,
        }
    }
}

impl Config {
    pub fn window_size(&self) -> (f64, f64) {
        (self.window_width.unwrap_or(800.0), self.window_height.unwrap_or(600.0))
    }

    pub fn window_position(&self) -> Option<(f64, f64)> {
        match (self.window_x, self.window_y) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        }
    }
}

fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("growterm")
}

fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        if path.exists() {
            return Self::load_from_file(&path);
        }

        // Migration: read legacy individual files
        let dir = config_dir();
        let has_legacy = dir.join("pomodoro_enabled").exists()
            || dir.join("response_timer_enabled").exists()
            || dir.join("coaching_enabled").exists()
            || dir.join("transparent_tab_bar").exists();

        if has_legacy {
            let config = Self::migrate_from_legacy(&dir);
            config.save();
            return config;
        }

        Self::default()
    }

    fn load_from_file(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    fn migrate_from_legacy(dir: &std::path::Path) -> Self {
        let read_bool = |name: &str, default: bool| -> bool {
            match std::fs::read_to_string(dir.join(name)) {
                Ok(s) => {
                    if default {
                        s.trim() != "0"
                    } else {
                        s.trim() == "1"
                    }
                }
                Err(_) => default,
            }
        };

        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            pomodoro: read_bool("pomodoro_enabled", false),
            response_timer: read_bool("response_timer_enabled", false),
            coaching: read_bool("coaching_enabled", true),
            transparent_tab_bar: read_bool("transparent_tab_bar", false),
            header_opacity: default_header_opacity(),
            coaching_command: None,
            copy_mode_keys: CopyModeKeys::default(),
            window_width: None,
            window_height: None,
            window_x: None,
            window_y: None,
        }
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let coaching_cmd_line = match &self.coaching_command {
            Some(cmd) => format!("coaching_command = {:?}\n", cmd),
            None => String::new(),
        };
        let mut window_lines = String::new();
        if let Some(w) = self.window_width {
            window_lines += &format!("window_width = {}\n", w);
        }
        if let Some(h) = self.window_height {
            window_lines += &format!("window_height = {}\n", h);
        }
        if let Some(x) = self.window_x {
            window_lines += &format!("window_x = {}\n", x);
        }
        if let Some(y) = self.window_y {
            window_lines += &format!("window_y = {}\n", y);
        }
        let content = format!(
            "font_family = {:?}\nfont_size = {}\npomodoro = {}\nresponse_timer = {}\ncoaching = {}\ntransparent_tab_bar = {}\nheader_opacity = {}\n{coaching_cmd_line}{window_lines}",
            self.font_family, self.font_size, self.pomodoro, self.response_timer, self.coaching, self.transparent_tab_bar, self.header_opacity,
        );
        let _ = std::fs::write(config_path(), content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let toml = r#"
font_family = "Menlo"
font_size = 24.0
pomodoro = true
response_timer = false
coaching = false
transparent_tab_bar = true
header_opacity = 0.5
coaching_command = "claude -p --system 'You are a coach' '{prompt}'"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.font_family, "Menlo");
        assert_eq!(config.font_size, 24.0);
        assert!(config.pomodoro);
        assert!(!config.response_timer);
        assert!(!config.coaching);
        assert!(config.transparent_tab_bar);
        assert_eq!(config.header_opacity, 0.5);
        assert_eq!(
            config.coaching_command,
            Some("claude -p --system 'You are a coach' '{prompt}'".to_string())
        );
    }

    #[test]
    fn coaching_command_default_is_none() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.coaching_command.is_none());
    }

    #[test]
    fn parse_empty_uses_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn parse_partial_config() {
        let toml = "font_size = 16.0\npomodoro = true\n";
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.font_size, 16.0);
        assert!(config.pomodoro);
        assert_eq!(config.font_family, "FiraCodeNerdFontMono-Retina");
        assert!(config.coaching); // default true
    }

    #[test]
    fn default_values() {
        let config = Config::default();
        assert_eq!(config.font_family, "FiraCodeNerdFontMono-Retina");
        assert_eq!(config.font_size, 32.0);
        assert!(!config.pomodoro);
        assert!(!config.response_timer);
        assert!(config.coaching);
        assert!(!config.transparent_tab_bar);
        assert_eq!(config.header_opacity, 0.8);
    }

    #[test]
    fn migrate_from_legacy_files() {
        let dir = std::env::temp_dir().join("growterm_test_migrate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("pomodoro_enabled"), "1").unwrap();
        std::fs::write(dir.join("response_timer_enabled"), "0").unwrap();
        std::fs::write(dir.join("coaching_enabled"), "0").unwrap();
        std::fs::write(dir.join("transparent_tab_bar"), "1").unwrap();

        let config = Config::migrate_from_legacy(&dir);
        assert!(config.pomodoro);
        assert!(!config.response_timer);
        assert!(!config.coaching);
        assert!(config.transparent_tab_bar);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_mode_keys_default() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.copy_mode_keys.down, vec!["j"]);
        assert_eq!(config.copy_mode_keys.exit, vec!["q", "Escape", "`"]);
    }

    #[test]
    fn copy_mode_keys_custom() {
        let toml = r#"
[copy_mode_keys]
down = "n"
half_page_down = ["d", "h"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.copy_mode_keys.down, vec!["n"]);
        assert_eq!(config.copy_mode_keys.half_page_down, vec!["d", "h"]);
        // defaults preserved for unspecified
        assert_eq!(config.copy_mode_keys.up, vec!["k"]);
    }

    #[test]
    fn copy_mode_keys_single_string_deserialized_as_vec() {
        let toml = r#"
[copy_mode_keys]
exit = "q"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.copy_mode_keys.exit, vec!["q"]);
    }

    #[test]
    fn build_action_map_default() {
        use growterm_macos::key_convert::keycode as kc;
        let keys = CopyModeKeys::default();
        let map = keys.build_action_map();
        assert_eq!(map.get(&kc::ANSI_J), Some(&CopyModeAction::Down));
        assert_eq!(map.get(&kc::ANSI_K), Some(&CopyModeAction::Up));
        assert_eq!(map.get(&kc::ANSI_V), Some(&CopyModeAction::Visual));
        assert_eq!(map.get(&kc::ANSI_H), Some(&CopyModeAction::HalfPageDown));
        assert_eq!(map.get(&kc::ANSI_D), Some(&CopyModeAction::HalfPageDown));
        assert_eq!(map.get(&kc::ANSI_L), Some(&CopyModeAction::HalfPageUp));
        assert_eq!(map.get(&kc::ANSI_U), Some(&CopyModeAction::HalfPageUp));
        assert_eq!(map.get(&kc::ANSI_Y), Some(&CopyModeAction::Yank));
        assert_eq!(map.get(&kc::ESCAPE), Some(&CopyModeAction::Exit));
        assert_eq!(map.get(&kc::ANSI_Q), Some(&CopyModeAction::Exit));
        assert_eq!(map.get(&kc::ANSI_GRAVE), Some(&CopyModeAction::Exit));
    }

    #[test]
    fn build_action_map_custom() {
        use growterm_macos::key_convert::keycode as kc;
        let mut keys = CopyModeKeys::default();
        keys.down = vec!["n".into()];
        let map = keys.build_action_map();
        assert_eq!(map.get(&kc::ANSI_N), Some(&CopyModeAction::Down));
        assert_eq!(map.get(&kc::ANSI_J), None); // j no longer mapped
    }

    #[test]
    fn window_size_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.window_size(), (800.0, 600.0));
        assert_eq!(config.window_position(), None);
    }

    #[test]
    fn window_size_and_position_from_config() {
        let toml = r#"
window_width = 1200
window_height = 800
window_x = 100
window_y = 50
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.window_size(), (1200.0, 800.0));
        assert_eq!(config.window_position(), Some((100.0, 50.0)));
    }

    #[test]
    fn window_position_requires_both_x_and_y() {
        let toml = "window_x = 100\n";
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.window_position(), None);
    }

    #[test]
    fn unknown_fields_ignored() {
        let toml = "font_size = 20.0\nunknown_field = 42\n";
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.font_size, 20.0);
    }
}
