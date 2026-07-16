use crate::Atlas;
use image::RgbaImage;
use quick_xml::{events::Event, Reader};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostumeOption {
    pub id: String,
    pub category: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub(crate) enum ExternalSource {
    Shimeji {
        actions: PathBuf,
        image_dir: PathBuf,
    },
    Ukagaka {
        shell_dir: PathBuf,
    },
    WlShimeji {
        package: PathBuf,
    },
    LooseImages {
        images: Vec<PathBuf>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ExternalPet {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub source: ExternalSource,
    pub costumes: Vec<CostumeOption>,
}

pub(crate) fn discover_external_pets(root: &Path) -> Vec<ExternalPet> {
    let mut pets = Vec::new();
    let mut directories = vec![(root.to_path_buf(), 0_usize)];
    while let Some((directory, depth)) = directories.pop() {
        if depth > 6 {
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        let mut children = Vec::new();
        let mut wlshm = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                children.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "wlshm")
            {
                wlshm.push(path);
            }
        }

        let conf_actions = directory.join("conf/actions.xml");
        let image_root = directory.join("img");
        if conf_actions.exists() && image_root.is_dir() {
            if let Ok(images) = fs::read_dir(&image_root) {
                for image_dir in images.flatten().filter_map(|entry| {
                    entry
                        .file_type()
                        .ok()
                        .filter(|kind| kind.is_dir())
                        .map(|_| entry.path())
                }) {
                    if contains_shimeji_images(&image_dir) {
                        let name = image_dir
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or("Shimeji");
                        pets.push(ExternalPet {
                            id: format!("shimeji-{}", slug(name)),
                            display_name: name.to_owned(),
                            description: "Imported Shimeji package".into(),
                            source: ExternalSource::Shimeji {
                                actions: conf_actions.clone(),
                                image_dir,
                            },
                            costumes: Vec::new(),
                        });
                    }
                }
            }
            children.retain(|path| path != &image_root);
        } else if directory.join("actions.xml").exists() && contains_shimeji_images(&directory) {
            let name = directory
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("Shimeji");
            pets.push(ExternalPet {
                id: format!("shimeji-{}", slug(name)),
                display_name: name.to_owned(),
                description: "Imported Shimeji package".into(),
                source: ExternalSource::Shimeji {
                    actions: directory.join("actions.xml"),
                    image_dir: directory.clone(),
                },
                costumes: Vec::new(),
            });
        } else if is_ukagaka_shell(&directory) {
            let metadata = parse_descript(&directory.join("descript.txt"));
            let name = metadata
                .get("name")
                .filter(|name| !name.eq_ignore_ascii_case("master"))
                .or_else(|| metadata.get("id"))
                .or_else(|| metadata.get("name"))
                .cloned()
                .or_else(|| {
                    directory
                        .file_name()
                        .and_then(|value| value.to_str())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "Ukagaka Shell".into());
            let images = surface_images(&directory);
            let surface_source =
                read_text_lossy(&directory.join("surfaces.txt")).unwrap_or_default();
            let overlays = parse_bind_overlays(&surface_source);
            let costumes = parse_costumes(&metadata)
                .into_iter()
                .filter(|costume| {
                    costume.id.parse::<u32>().ok().is_some_and(|group| {
                        overlays.get(&group).is_some_and(|parts| {
                            parts
                                .iter()
                                .any(|(surface, _, _)| images.contains_key(surface))
                        })
                    })
                })
                .collect();
            pets.push(ExternalPet {
                id: format!("ukagaka-{}", slug(&name)),
                display_name: name,
                description: "Imported Ukagaka shell (visual assets only)".into(),
                costumes,
                source: ExternalSource::Ukagaka {
                    shell_dir: directory.clone(),
                },
            });
            children.clear();
        } else if is_ukagaka_bitmap_directory(&directory) {
            let images = png_images(&directory);
            let name = directory
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .and_then(|value| value.to_str())
                .map(|value| format!("{value} Visual Pet"))
                .unwrap_or_else(|| "Ukagaka Visual Pet".into());
            pets.push(ExternalPet {
                id: format!("visual-{}", slug(&name)),
                display_name: name,
                description: "Imported loose Ukagaka visual assets".into(),
                source: ExternalSource::LooseImages { images },
                costumes: Vec::new(),
            });
            children.clear();
        }

        for package in wlshm {
            if let Some(name) = wlshm_name(&package) {
                pets.push(ExternalPet {
                    id: format!("wlshimeji-{}", slug(&name)),
                    display_name: name,
                    description: "Imported wl_shimeji package".into(),
                    source: ExternalSource::WlShimeji { package },
                    costumes: Vec::new(),
                });
            }
        }
        directories.extend(children.into_iter().map(|child| (child, depth + 1)));
    }
    pets
}

pub(crate) fn load_external_atlas(
    source: &ExternalSource,
    selected_costumes: &[String],
) -> Result<Atlas, String> {
    match source {
        ExternalSource::Shimeji { actions, image_dir } => load_shimeji(actions, image_dir),
        ExternalSource::Ukagaka { shell_dir } => load_ukagaka(shell_dir, selected_costumes),
        ExternalSource::WlShimeji { package } => load_wlshimeji(package),
        ExternalSource::LooseImages { images } => load_loose_images(images),
    }
}

fn load_loose_images(images: &[PathBuf]) -> Result<Atlas, String> {
    let frames: Vec<RgbaImage> = images
        .iter()
        .take(8)
        .map(|path| {
            image::open(path)
                .map(|image| image.into_rgba8())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<_, _>>()?;
    if frames.is_empty() {
        return Err("loose visual package contains no PNG images".into());
    }
    let rows = vec![
        frames.clone(),
        mirror_frames(&frames),
        frames.clone(),
        frames.clone(),
        frames.clone(),
        frames.clone(),
        frames.clone(),
        frames.clone(),
        frames.clone(),
        frames.clone(),
        frames,
    ];
    Ok(Atlas::from_state_frames(&rows))
}

fn load_shimeji(actions: &Path, image_dir: &Path) -> Result<Atlas, String> {
    let action_frames = parse_shimeji_actions(actions)?;
    let fallback: Vec<String> = action_frames.values().flatten().cloned().collect();
    let idle = select_named(
        &action_frames,
        &["stand", "sit", "sprawl", "idle"],
        &fallback,
    );
    let walk = select_named(&action_frames, &["walk", "run", "dash", "creep"], &idle);
    let jump = select_named(
        &action_frames,
        &["jump", "bounce", "falling", "dragged", "pinched"],
        &idle,
    );
    let failed = select_named(&action_frames, &["trip", "fall", "dispose"], &jump);
    let waiting = select_named(&action_frames, &["lookatmouse", "look", "sit"], &idle);
    let waving = select_named(&action_frames, &["wave", "greet", "spinhead"], &waiting);
    let review = select_named(&action_frames, &["wave", "look", "stand"], &idle);

    let idle_images = load_named_images(image_dir, &idle)?;
    let left_images = load_named_images(image_dir, &walk)?;
    let right_images = mirror_frames(&left_images);
    let rows = vec![
        idle_images.clone(),
        right_images,
        left_images.clone(),
        load_named_images(image_dir, &waving)?,
        load_named_images(image_dir, &jump)?,
        load_named_images(image_dir, &failed)?,
        load_named_images(image_dir, &waiting)?,
        left_images,
        load_named_images(image_dir, &review)?,
        idle_images.clone(),
        idle_images,
    ];
    Ok(Atlas::from_state_frames(&rows))
}

fn load_wlshimeji(package: &Path) -> Result<Atlas, String> {
    let bytes = fs::read(package).map_err(|error| error.to_string())?;
    if bytes.len() <= 512 || &bytes[..4] != b"WLPK" {
        return Err("invalid wl_shimeji package header".into());
    }
    let mut archive = tar::Archive::new(Cursor::new(&bytes[512..]));
    let mut actions = None;
    let mut assets = HashMap::new();
    for entry in archive.entries().map_err(|error| error.to_string())? {
        let mut entry = entry.map_err(|error| error.to_string())?;
        let path = entry
            .path()
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .map_err(|error| error.to_string())?;
        if path == "actions.json" {
            actions = serde_json::from_slice::<Value>(&data).ok();
        } else if path.starts_with("assets/") && path.ends_with(".qoi") {
            assets.insert(path.trim_start_matches("assets/").to_owned(), data);
        }
    }
    let actions = actions.ok_or_else(|| "wl_shimeji actions.json is missing".to_owned())?;
    let action_frames = parse_wl_actions(&actions);
    let fallback: Vec<String> = action_frames.values().flatten().cloned().collect();
    let decode = |names: &[String]| -> Result<Vec<RgbaImage>, String> {
        let mut frames = Vec::new();
        for name in names.iter().take(8) {
            if let Some(data) = assets.get(name.trim_start_matches('/')) {
                frames.push(
                    image::load_from_memory(data)
                        .map_err(|error| error.to_string())?
                        .into_rgba8(),
                );
            }
        }
        if frames.is_empty() {
            Err("wl_shimeji action contains no decodable frames".into())
        } else {
            Ok(frames)
        }
    };
    let idle_names = select_named(&action_frames, &["stand", "sit", "idle"], &fallback);
    let walk_names = select_named(&action_frames, &["walk", "run", "dash"], &idle_names);
    let idle = decode(&idle_names)?;
    let left = decode(&walk_names)?;
    let rows = vec![
        idle.clone(),
        mirror_frames(&left),
        left.clone(),
        decode(&select_named(
            &action_frames,
            &["wave", "look"],
            &idle_names,
        ))?,
        decode(&select_named(
            &action_frames,
            &["jump", "bounce", "dragged", "pinched"],
            &idle_names,
        ))?,
        decode(&select_named(
            &action_frames,
            &["trip", "fall"],
            &idle_names,
        ))?,
        decode(&select_named(&action_frames, &["sit", "look"], &idle_names))?,
        left,
        decode(&select_named(
            &action_frames,
            &["wave", "stand"],
            &idle_names,
        ))?,
        idle.clone(),
        idle,
    ];
    Ok(Atlas::from_state_frames(&rows))
}

fn load_ukagaka(shell_dir: &Path, selected_costumes: &[String]) -> Result<Atlas, String> {
    let surfaces_source = read_text_lossy(&shell_dir.join("surfaces.txt")).unwrap_or_default();
    let animations = parse_surface_animations(&surfaces_source);
    let overlays = parse_bind_overlays(&surfaces_source);
    let selected: Vec<u32> = selected_costumes
        .iter()
        .filter_map(|value| value.parse().ok())
        .collect();
    let available = surface_images(shell_dir);
    if available.is_empty() {
        return Err("Ukagaka shell contains no surface PNG files".into());
    }
    let fallback_ids: Vec<u32> = available.keys().copied().take(8).collect();
    let sequence = |base: u32| {
        let mut ids = vec![base];
        if let Some(extra) = animations.get(&base) {
            ids.extend(extra.iter().copied());
        }
        ids.retain(|id| available.contains_key(id));
        if ids.is_empty() {
            fallback_ids.clone()
        } else {
            ids
        }
    };
    let load = |ids: Vec<u32>| -> Result<Vec<RgbaImage>, String> {
        ids.into_iter()
            .take(8)
            .map(|id| {
                let path = available
                    .get(&id)
                    .ok_or_else(|| format!("surface{id} is missing"))?;
                let mut base = image::open(path)
                    .map_err(|error| error.to_string())?
                    .into_rgba8();
                for costume in &selected {
                    if let Some(parts) = overlays.get(costume) {
                        for (overlay_id, x, y) in parts {
                            if let Some(overlay_path) = available.get(overlay_id) {
                                let overlay = image::open(overlay_path)
                                    .map_err(|error| error.to_string())?
                                    .into_rgba8();
                                image::imageops::overlay(
                                    &mut base,
                                    &overlay,
                                    i64::from(*x),
                                    i64::from(*y),
                                );
                            }
                        }
                    }
                }
                Ok(base)
            })
            .collect()
    };
    let idle = load(sequence(0))?;
    let movement = load(first_sequence(
        &available,
        &animations,
        &[20, 16, 9, 0],
        &fallback_ids,
    ))?;
    let rows = vec![
        idle.clone(),
        mirror_frames(&movement),
        movement.clone(),
        load(first_sequence(
            &available,
            &animations,
            &[5, 20, 2, 0],
            &fallback_ids,
        ))?,
        load(first_sequence(
            &available,
            &animations,
            &[9, 16, 2, 0],
            &fallback_ids,
        ))?,
        load(first_sequence(
            &available,
            &animations,
            &[4, 3, 1, 0],
            &fallback_ids,
        ))?,
        load(first_sequence(
            &available,
            &animations,
            &[3, 1, 0],
            &fallback_ids,
        ))?,
        movement,
        load(first_sequence(
            &available,
            &animations,
            &[2, 1, 0],
            &fallback_ids,
        ))?,
        idle.clone(),
        idle,
    ];
    Ok(Atlas::from_state_frames(&rows))
}

fn parse_shimeji_actions(path: &Path) -> Result<HashMap<String, Vec<String>>, String> {
    let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let mut reader = Reader::from_str(&source);
    reader.config_mut().trim_text(true);
    let mut current = None;
    let mut actions: HashMap<String, Vec<String>> = HashMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) if element.name().as_ref() == b"Action" => {
                current = element
                    .attributes()
                    .flatten()
                    .find(|attribute| attribute.key.as_ref() == b"Name")
                    .and_then(|attribute| attribute.unescape_value().ok())
                    .map(|value| value.to_string());
            }
            Ok(Event::Empty(element)) if element.name().as_ref() == b"Pose" => {
                if let Some(action) = &current {
                    for attribute in element.attributes().flatten() {
                        if matches!(attribute.key.as_ref(), b"Image" | b"ImageRight") {
                            if let Ok(value) = attribute.unescape_value() {
                                actions
                                    .entry(action.to_lowercase())
                                    .or_default()
                                    .push(value.trim_start_matches('/').to_owned());
                            }
                        }
                    }
                }
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"Action" => current = None,
            Ok(Event::Eof) => break,
            Err(error) => return Err(error.to_string()),
            _ => {}
        }
    }
    if actions.is_empty() {
        Err("Shimeji actions.xml contains no image poses".into())
    } else {
        Ok(actions)
    }
}

fn parse_wl_actions(actions: &Value) -> HashMap<String, Vec<String>> {
    let mut result = HashMap::new();
    if let Some(object) = actions.as_object() {
        for (name, action) in object {
            let mut images = Vec::new();
            collect_json_images(action, &mut images);
            if !images.is_empty() {
                result.insert(name.to_lowercase(), images);
            }
        }
    }
    result
}

fn collect_json_images(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.as_str(), "image" | "image_right") {
                    if let Some(image) = value.as_str() {
                        output.push(image.to_owned());
                    }
                } else {
                    collect_json_images(value, output);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_json_images(value, output);
            }
        }
        _ => {}
    }
}

fn select_named(
    actions: &HashMap<String, Vec<String>>,
    keywords: &[&str],
    fallback: &[String],
) -> Vec<String> {
    for keyword in keywords {
        if let Some((_, frames)) = actions
            .iter()
            .find(|(name, _)| name.contains(&keyword.to_lowercase()))
        {
            if !frames.is_empty() {
                return frames.clone();
            }
        }
    }
    fallback.iter().take(8).cloned().collect()
}

fn load_named_images(directory: &Path, names: &[String]) -> Result<Vec<RgbaImage>, String> {
    let mut frames = Vec::new();
    for name in names.iter().take(8) {
        let path = directory.join(name.trim_start_matches('/'));
        if path.exists() {
            frames.push(
                image::open(&path)
                    .map_err(|error| error.to_string())?
                    .into_rgba8(),
            );
        }
    }
    if frames.is_empty() {
        Err(format!(
            "no referenced images found in {}",
            directory.display()
        ))
    } else {
        Ok(frames)
    }
}

fn mirror_frames(frames: &[RgbaImage]) -> Vec<RgbaImage> {
    frames
        .iter()
        .map(image::imageops::flip_horizontal)
        .collect()
}

fn parse_descript(path: &Path) -> BTreeMap<String, String> {
    let source = read_text_lossy(path).unwrap_or_default();
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once(',')?;
            Some((key.trim().to_lowercase(), value.trim().to_owned()))
        })
        .collect()
}

fn parse_costumes(metadata: &BTreeMap<String, String>) -> Vec<CostumeOption> {
    metadata
        .iter()
        .filter_map(|(key, value)| {
            let id = key
                .strip_prefix("sakura.bindgroup")?
                .strip_suffix(".name")?;
            let mut parts = value.splitn(2, ',');
            Some(CostumeOption {
                id: id.to_owned(),
                category: parts.next().unwrap_or("Costume").trim().to_owned(),
                name: parts.next().unwrap_or(value).trim().to_owned(),
            })
        })
        .collect()
}

fn surface_images(directory: &Path) -> BTreeMap<u32, PathBuf> {
    let mut images = BTreeMap::new();
    let Ok(entries) = fs::read_dir(directory) else {
        return images;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower = name.to_lowercase();
        if lower.starts_with("surface") && lower.ends_with(".png") && !lower.contains('_') {
            if let Ok(id) = lower[7..lower.len() - 4].parse::<u32>() {
                images.insert(id, path);
            }
        }
    }
    images
}

fn parse_surface_animations(source: &str) -> HashMap<u32, Vec<u32>> {
    let mut current_surface = None;
    let mut animations: HashMap<u32, Vec<u32>> = HashMap::new();
    for line in source.lines() {
        let line = line.trim().to_lowercase();
        if let Some(id) = line
            .strip_prefix("surface")
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<u32>().ok())
        {
            current_surface = Some(id);
        } else if line.starts_with('}') {
            current_surface = None;
        } else if line.contains(".pattern") && line.contains(",replace,") {
            if let (Some(surface), Some((_, values))) = (current_surface, line.split_once(',')) {
                let parts: Vec<&str> = values.split(',').collect();
                if let Some(id) = parts.get(1).and_then(|value| value.parse::<i32>().ok()) {
                    if id >= 0 {
                        animations.entry(surface).or_default().push(id as u32);
                    }
                }
            }
        }
    }
    animations
}

fn parse_bind_overlays(source: &str) -> HashMap<u32, Vec<(u32, i32, i32)>> {
    let mut overlays: HashMap<u32, Vec<(u32, i32, i32)>> = HashMap::new();
    for line in source.lines().map(str::trim) {
        let lower = line.to_lowercase();
        let Some(animation) = lower.strip_prefix("animation") else {
            continue;
        };
        let Some((group, rest)) = animation.split_once(".pattern") else {
            continue;
        };
        let Ok(group) = group.parse::<u32>() else {
            continue;
        };
        let Some((_, values)) = rest.split_once(',') else {
            continue;
        };
        let parts: Vec<&str> = values.split(',').collect();
        if parts.first() != Some(&"bind") {
            continue;
        }
        if let (Some(surface), Some(x), Some(y)) = (
            parts.get(1).and_then(|value| value.parse::<u32>().ok()),
            parts.get(3).and_then(|value| value.parse::<i32>().ok()),
            parts.get(4).and_then(|value| value.parse::<i32>().ok()),
        ) {
            let entry = overlays.entry(group).or_default();
            if !entry.iter().any(|value| value.0 == surface) {
                entry.push((surface, x, y));
            }
        }
    }
    overlays
}

fn first_sequence(
    images: &BTreeMap<u32, PathBuf>,
    animations: &HashMap<u32, Vec<u32>>,
    candidates: &[u32],
    fallback: &[u32],
) -> Vec<u32> {
    for candidate in candidates {
        if images.contains_key(candidate) {
            let mut sequence = vec![*candidate];
            if let Some(extra) = animations.get(candidate) {
                sequence.extend(extra.iter().copied());
            }
            sequence.retain(|id| images.contains_key(id));
            return sequence;
        }
    }
    fallback.to_vec()
}

fn read_text_lossy(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if let Ok(source) = String::from_utf8(bytes.clone()) {
        return Some(source);
    }
    let (source, _, _) = encoding_rs::SHIFT_JIS.decode(&bytes);
    Some(source.into_owned())
}

fn png_images(directory: &Path) -> Vec<PathBuf> {
    let mut images: Vec<PathBuf> = fs::read_dir(directory)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("png"))
        })
        .collect();
    images.sort();
    images
}

fn is_ukagaka_bitmap_directory(directory: &Path) -> bool {
    directory
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("bitmap"))
        && directory
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("resources"))
        && png_images(directory).len() >= 2
}

fn is_ukagaka_shell(directory: &Path) -> bool {
    directory.join("descript.txt").exists()
        && directory.join("surfaces.txt").exists()
        && surface_images(directory).len() >= 2
}

fn contains_shimeji_images(directory: &Path) -> bool {
    fs::read_dir(directory).ok().is_some_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("shime") && name.ends_with(".png"))
        })
    })
}

fn wlshm_name(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() < 6 || &bytes[..4] != b"WLPK" {
        return None;
    }
    let length = usize::from(bytes[4]);
    String::from_utf8(bytes.get(5..5 + length)?.to_vec()).ok()
}

fn slug(value: &str) -> String {
    let slug: String = value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    slug.trim_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ukagaka_costumes_and_bind_overlays() {
        let metadata = BTreeMap::from([
            ("name".into(), "茶兔".into()),
            ("sakura.bindgroup50.name".into(), "衣服,圣诞衣".into()),
        ]);
        assert_eq!(parse_costumes(&metadata)[0].name, "圣诞衣");
        let overlays = parse_bind_overlays(
            "animation50.pattern0,bind,3010,2,4,6\nanimation50.pattern1,bind,3010,2,4,6",
        );
        assert_eq!(overlays[&50], vec![(3010, 4, 6)]);
    }

    #[test]
    fn discovers_and_normalizes_ukagaka_shell_with_costume() {
        let root = tempfile::tempdir().unwrap();
        let shell = root.path().join("tea-rabbit");
        fs::create_dir_all(&shell).unwrap();
        fs::write(
            shell.join("descript.txt"),
            "name,茶兔\nsakura.bindgroup50.name,衣服,测试服\n",
        )
        .unwrap();
        fs::write(
            shell.join("surfaces.txt"),
            "surface0\n{\nanimation50.pattern0,bind,100,0,0,0\n}\n",
        )
        .unwrap();
        let mut base = RgbaImage::new(32, 32);
        base.put_pixel(16, 16, image::Rgba([255, 0, 0, 255]));
        base.save(shell.join("surface0.png")).unwrap();
        base.save(shell.join("surface1.png")).unwrap();
        let mut overlay = RgbaImage::new(32, 32);
        overlay.put_pixel(16, 16, image::Rgba([0, 0, 255, 255]));
        overlay.save(shell.join("surface100.png")).unwrap();

        let pets = discover_external_pets(root.path());
        assert_eq!(pets.len(), 1);
        assert_eq!(pets[0].costumes.len(), 1);
        let default = load_external_atlas(&pets[0].source, &[]).unwrap();
        let dressed = load_external_atlas(&pets[0].source, &["50".into()]).unwrap();
        assert_ne!(default.frame_rgba(0, 0), dressed.frame_rgba(0, 0));
    }

    #[test]
    fn discovers_and_normalizes_shimeji_directory() {
        let root = tempfile::tempdir().unwrap();
        let conf = root.path().join("conf");
        let images = root.path().join("img/TestMascot");
        fs::create_dir_all(&conf).unwrap();
        fs::create_dir_all(&images).unwrap();
        fs::write(
            conf.join("actions.xml"),
            r#"<Mascot><ActionList><Action Name="Stand"><Animation><Pose Image="/shime1.png"/></Animation></Action><Action Name="Walk"><Animation><Pose Image="/shime2.png"/></Animation></Action></ActionList></Mascot>"#,
        )
        .unwrap();
        RgbaImage::from_pixel(32, 32, image::Rgba([255, 0, 0, 255]))
            .save(images.join("shime1.png"))
            .unwrap();
        RgbaImage::from_pixel(32, 32, image::Rgba([0, 255, 0, 255]))
            .save(images.join("shime2.png"))
            .unwrap();
        let pets = discover_external_pets(root.path());
        assert_eq!(pets.len(), 1);
        load_external_atlas(&pets[0].source, &[]).unwrap();
    }

    #[test]
    fn discovers_and_normalizes_wlshimeji_package() {
        let root = tempfile::tempdir().unwrap();
        let mut qoi = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            32,
            32,
            image::Rgba([20, 180, 220, 255]),
        ))
        .write_to(&mut qoi, image::ImageFormat::Qoi)
        .unwrap();
        let mut archive_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut archive_bytes);
            for (name, data) in [
                (
                    "actions.json",
                    br#"{"Stand":{"animations":[{"frames":[{"image":"shime1.qoi"}]}]},"Walk":{"animations":[{"frames":[{"image":"shime1.qoi"}]}]}}"#.as_slice(),
                ),
                ("assets/shime1.qoi", qoi.get_ref().as_slice()),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, name, data).unwrap();
            }
            builder.finish().unwrap();
        }
        let name = "Test WL";
        let mut package = vec![0_u8; 512];
        package[..4].copy_from_slice(b"WLPK");
        package[4] = name.len() as u8;
        package[5..5 + name.len()].copy_from_slice(name.as_bytes());
        package.extend(archive_bytes);
        fs::write(root.path().join("Test.wlshm"), package).unwrap();
        let pets = discover_external_pets(root.path());
        assert_eq!(pets.len(), 1);
        load_external_atlas(&pets[0].source, &[]).unwrap();
    }

    #[test]
    fn parses_shimeji_action_pose_images() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("actions.xml");
        fs::write(
            &path,
            r#"<Mascot><ActionList><Action Name="Walk"><Animation><Pose Image="/shime1.png"/><Pose Image="/shime2.png"/></Animation></Action></ActionList></Mascot>"#,
        )
        .unwrap();
        assert_eq!(
            parse_shimeji_actions(&path).unwrap()["walk"],
            vec!["shime1.png", "shime2.png"]
        );
    }
}
