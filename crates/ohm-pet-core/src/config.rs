use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub selected_pet_id: String,
    pub scale: f32,
    pub autonomous: bool,
    pub always_on_top: bool,
    pub window_x: Option<i32>,
    pub window_y: Option<i32>,
    pub selected_costumes: HashMap<String, Vec<String>>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            selected_pet_id: "ohm-raven".into(),
            scale: 0.62,
            autonomous: true,
            always_on_top: true,
            window_x: None,
            window_y: None,
            selected_costumes: HashMap::new(),
        }
    }
}

pub struct PreferencesStore {
    path: PathBuf,
}

impl PreferencesStore {
    pub fn system() -> Option<Self> {
        ProjectDirs::from("works", "Earendil", "OHM Pet").map(|dirs| Self {
            path: dirs.config_dir().join("preferences.json"),
        })
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Preferences {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|source| serde_json::from_str(&source).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, preferences: &Preferences) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let source = serde_json::to_string_pretty(preferences).map_err(io::Error::other)?;
        fs::write(&self.path, source)
    }
}
