use crate::layout::Preset;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct Config {
    #[serde(default)]
    pub frame: Vec<FrameConfig>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct FrameConfig {
    pub preset: Preset,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub maximized: bool,
}

impl Default for FrameConfig {
    fn default() -> Self {
        Self { preset: Preset::Grid2x2, x: 100, y: 100, width: 1280, height: 800, maximized: false }
    }
}

/// exe と同じフォルダの config.toml(ポータブル運用)
pub fn config_path() -> PathBuf {
    let mut p = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("window-panel-tool.exe"));
    p.set_file_name("config.toml");
    p
}

/// 欠損・破損時はデフォルト(仕様8: 上書き保存で自己修復)
pub fn load(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(s) => toml::from_str(&s).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

pub fn save(path: &Path, cfg: &Config) -> std::io::Result<()> {
    let s = toml::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    std::fs::write(path, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Preset;

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("wpt-test-{}-{}", std::process::id(), name));
        p
    }

    #[test]
    fn roundtrip_preserves_config() {
        let path = tmp("roundtrip.toml");
        let cfg = Config {
            frame: vec![FrameConfig {
                preset: Preset::Cols2,
                x: 10,
                y: 20,
                width: 800,
                height: 600,
                maximized: true,
            }],
        };
        save(&path, &cfg).unwrap();
        assert_eq!(load(&path), cfg);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn missing_file_returns_default() {
        assert_eq!(load(std::path::Path::new("Z:/no/such/file.toml")), Config::default());
    }

    #[test]
    fn corrupt_file_returns_default() {
        let path = tmp("corrupt.toml");
        std::fs::write(&path, "this is {{ not toml !!").unwrap();
        assert_eq!(load(&path), Config::default());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn maximized_defaults_to_false_when_absent() {
        let path = tmp("nomax.toml");
        std::fs::write(
            &path,
            "[[frame]]\npreset = \"grid2x2\"\nx = 1\ny = 2\nwidth = 3\nheight = 4\n",
        )
        .unwrap();
        let cfg = load(&path);
        assert!(!cfg.frame[0].maximized);
        assert_eq!(cfg.frame[0].preset, Preset::Grid2x2);
        std::fs::remove_file(&path).ok();
    }
}
