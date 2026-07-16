use crate::Atlas;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PetManifest {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub sprite_version_number: u32,
    pub spritesheet_path: String,
}

#[derive(Debug, Clone)]
pub struct Pet {
    pub manifest: PetManifest,
    pub directory: PathBuf,
}

impl Pet {
    pub fn load_atlas(&self) -> Result<Atlas, crate::AtlasError> {
        Atlas::load(self.directory.join(&self.manifest.spritesheet_path))
    }
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("failed to read pet catalog: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid manifest {path}: {source}")]
    Manifest {
        path: PathBuf,
        source: serde_json::Error,
    },
}

pub struct PetCatalog {
    pets: Vec<Pet>,
}

impl PetCatalog {
    pub fn scan(root: impl AsRef<Path>) -> Result<Self, CatalogError> {
        let mut pets = Vec::new();
        if !root.as_ref().exists() {
            return Ok(Self { pets });
        }
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path();
            let manifest_path = path.join("pet.json");
            if !manifest_path.exists() {
                continue;
            }
            let source = fs::read_to_string(&manifest_path)?;
            let manifest: PetManifest =
                serde_json::from_str(&source).map_err(|source| CatalogError::Manifest {
                    path: manifest_path,
                    source,
                })?;
            if manifest.sprite_version_number == 2 && path.join(&manifest.spritesheet_path).exists()
            {
                pets.push(Pet {
                    manifest,
                    directory: path,
                });
            }
        }
        pets.sort_by(|left, right| left.manifest.display_name.cmp(&right.manifest.display_name));
        Ok(Self { pets })
    }

    pub fn pets(&self) -> &[Pet] {
        &self.pets
    }

    pub fn find(&self, id: &str) -> Option<&Pet> {
        self.pets.iter().find(|pet| pet.manifest.id == id)
    }
}
