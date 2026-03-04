use serde::Deserialize;
use std::path::PathBuf;

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
        }
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let coaching_cmd_line = match &self.coaching_command {
            Some(cmd) => format!("coaching_command = {:?}\n", cmd),
            None => String::new(),
        };
        let content = format!(
            "font_family = {:?}\nfont_size = {}\npomodoro = {}\nresponse_timer = {}\ncoaching = {}\ntransparent_tab_bar = {}\nheader_opacity = {}\n{coaching_cmd_line}",
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
    fn unknown_fields_ignored() {
        let toml = "font_size = 20.0\nunknown_field = 42\n";
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.font_size, 20.0);
    }
}
