use crate::{
    external::{discover_external_pets, load_external_atlas, CostumeOption, ExternalSource},
    Atlas, AtlasError,
};
use serde::Deserialize;
use std::{
    collections::HashSet,
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
enum PetSource {
    Codex,
    External(ExternalSource),
}

#[derive(Debug, Clone)]
pub struct Pet {
    pub manifest: PetManifest,
    pub directory: PathBuf,
    pub costumes: Vec<CostumeOption>,
    source: PetSource,
}

impl Pet {
    pub fn load_atlas(&self) -> Result<Atlas, PetLoadError> {
        self.load_atlas_with_costumes(&[])
    }

    pub fn load_atlas_with_costumes(&self, costumes: &[String]) -> Result<Atlas, PetLoadError> {
        match &self.source {
            PetSource::Codex => Atlas::load(self.directory.join(&self.manifest.spritesheet_path))
                .map_err(PetLoadError::Atlas),
            PetSource::External(source) => {
                load_external_atlas(source, costumes).map_err(PetLoadError::External)
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum PetLoadError {
    #[error(transparent)]
    Atlas(#[from] AtlasError),
    #[error("failed to normalize external pet: {0}")]
    External(String),
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("failed to read pet catalog: {0}")]
    Io(#[from] std::io::Error),
}

pub struct PetCatalog {
    pets: Vec<Pet>,
}

impl PetCatalog {
    pub fn scan(root: impl AsRef<Path>) -> Result<Self, CatalogError> {
        Self::scan_many([root.as_ref()])
    }

    pub fn scan_many<I, P>(roots: I) -> Result<Self, CatalogError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut pets = Vec::new();
        let mut seen_ids = HashSet::new();
        for root in roots {
            let root = root.as_ref();
            if !root.exists() {
                continue;
            }
            for entry in fs::read_dir(root)? {
                let Ok(entry) = entry else {
                    continue;
                };
                if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let path = entry.path();
                let manifest_path = path.join("pet.json");
                if !manifest_path.exists() {
                    continue;
                }
                let Ok(source) = fs::read_to_string(&manifest_path) else {
                    continue;
                };
                let Ok(manifest) = serde_json::from_str::<PetManifest>(&source) else {
                    continue;
                };
                if manifest.sprite_version_number == 2
                    && path.join(&manifest.spritesheet_path).exists()
                    && seen_ids.insert(manifest.id.clone())
                {
                    pets.push(Pet {
                        manifest,
                        directory: path,
                        costumes: Vec::new(),
                        source: PetSource::Codex,
                    });
                }
            }
            for external in discover_external_pets(root) {
                if seen_ids.insert(external.id.clone()) {
                    pets.push(Pet {
                        manifest: PetManifest {
                            id: external.id,
                            display_name: external.display_name,
                            description: external.description,
                            sprite_version_number: 0,
                            spritesheet_path: String::new(),
                        },
                        directory: root.to_path_buf(),
                        costumes: external.costumes,
                        source: PetSource::External(external.source),
                    });
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pet(root: &Path, folder: &str, id: &str, name: &str) {
        let directory = root.join(folder);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("spritesheet.webp"), []).unwrap();
        fs::write(
            directory.join("pet.json"),
            format!(
                r#"{{"id":"{id}","displayName":"{name}","description":"test","spriteVersionNumber":2,"spritesheetPath":"spritesheet.webp"}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn merges_roots_and_keeps_the_highest_priority_duplicate() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        write_pet(first.path(), "preferred", "shared", "Local Shared");
        write_pet(second.path(), "duplicate", "shared", "Codex Shared");
        write_pet(second.path(), "extra", "extra", "Codex Extra");

        let catalog = PetCatalog::scan_many([first.path(), second.path()]).unwrap();
        assert_eq!(catalog.pets().len(), 2);
        assert_eq!(
            catalog.find("shared").unwrap().manifest.display_name,
            "Local Shared"
        );
        assert!(catalog.find("extra").is_some());
    }
}
