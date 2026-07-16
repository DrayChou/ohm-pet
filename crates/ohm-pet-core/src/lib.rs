mod atlas;
mod behavior;
mod catalog;
mod config;
mod state;

pub use atlas::{Atlas, AtlasError, CELL_HEIGHT, CELL_WIDTH, COLUMNS, ROWS};
pub use behavior::{BehaviorBrain, BehaviorContext, BehaviorDecision};
pub use catalog::{Pet, PetCatalog, PetManifest};
pub use config::{Preferences, PreferencesStore};
pub use state::{
    direction_from_vector, frame_coordinates, AnimationState, FrameCoordinates, StateMachine,
};
