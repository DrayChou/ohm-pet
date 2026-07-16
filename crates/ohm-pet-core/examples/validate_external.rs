use ohm_pet_core::PetCatalog;
use std::{env, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let root = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: validate_external <pet-directory>")?;
    let catalog = PetCatalog::scan(&root)?;
    if catalog.pets().is_empty() {
        return Err(format!("no compatible pets found under {}", root.display()).into());
    }
    for pet in catalog.pets() {
        let default_atlas = pet.load_atlas()?;
        println!(
            "ok\t{}\t{}\t{} costume options",
            pet.manifest.id,
            pet.manifest.display_name,
            pet.costumes.len()
        );
        for costume in &pet.costumes {
            let costume_atlas = pet.load_atlas_with_costumes(std::slice::from_ref(&costume.id))?;
            if costume_atlas.frame_rgba(0, 0) == default_atlas.frame_rgba(0, 0) {
                return Err(format!(
                    "costume {} on {} did not change the rendered idle frame",
                    costume.id, pet.manifest.display_name
                )
                .into());
            }
            println!(
                "  costume ok\t{}\t{}：{}",
                costume.id, costume.category, costume.name
            );
        }
    }
    Ok(())
}
