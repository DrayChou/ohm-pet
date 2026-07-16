use anyhow::{anyhow, Context, Result};
use ohm_pet_core::{
    direction_from_vector, AnimationState, Atlas, BehaviorBrain, BehaviorContext, PetCatalog,
    Preferences, PreferencesStore, StateMachine, CELL_HEIGHT, CELL_WIDTH,
};
#[cfg(target_os = "macos")]
mod macos_renderer;

#[cfg(target_os = "macos")]
use macos_renderer::NativeRenderer;

#[cfg(target_os = "windows")]
mod windows_renderer;

#[cfg(target_os = "windows")]
use windows_renderer::NativeRenderer;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    Icon, TrayIcon, TrayIconBuilder,
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId, WindowLevel},
};

struct DesktopPet {
    window: Option<Arc<Window>>,
    renderer: Option<NativeRenderer>,
    atlas: Option<Atlas>,
    state: StateMachine,
    preferences: Preferences,
    preferences_store: Option<PreferencesStore>,
    pets_root: PathBuf,
    next_autonomous_at: Instant,
    next_pointer_sample_at: Instant,
    tray: Option<TrayIcon>,
    press_window_position: Option<PhysicalPosition<i32>>,
    brain: BehaviorBrain,
    last_interaction_at: Instant,
    recent_interactions: u32,
    pointer_nearby: bool,
}

impl DesktopPet {
    fn new(pets_root: PathBuf) -> Self {
        let preferences_store = PreferencesStore::system();
        let preferences = preferences_store
            .as_ref()
            .map_or_else(Preferences::default, PreferencesStore::load);
        let now = Instant::now();
        Self {
            window: None,
            renderer: None,
            atlas: None,
            state: StateMachine::new(now),
            preferences,
            preferences_store,
            pets_root,
            next_autonomous_at: now + Duration::from_secs(14),
            next_pointer_sample_at: now,
            tray: None,
            press_window_position: None,
            brain: BehaviorBrain::default(),
            last_interaction_at: now,
            recent_interactions: 0,
            pointer_nearby: false,
        }
    }

    fn load_selected_pet(&mut self) -> Result<()> {
        let catalog = PetCatalog::scan(&self.pets_root).context("scan pet catalog")?;
        let pet = catalog
            .find(&self.preferences.selected_pet_id)
            .or_else(|| catalog.pets().first())
            .ok_or_else(|| anyhow!("no valid pets found in {}", self.pets_root.display()))?;
        self.preferences.selected_pet_id = pet.manifest.id.clone();
        self.atlas = Some(pet.load_atlas().context("load selected pet atlas")?);
        if let Some(store) = &self.preferences_store {
            let _ = store.save(&self.preferences);
        }
        Ok(())
    }

    fn render(&mut self) {
        let (Some(renderer), Some(atlas)) = (&mut self.renderer, &self.atlas) else {
            return;
        };
        let coordinates = self.state.coordinates();
        if let Err(error) = renderer.render(atlas, coordinates.row, coordinates.column) {
            eprintln!("render error: {error:#}");
        }
    }

    fn update_pointer_gaze(&mut self, now: Instant) -> bool {
        if now < self.next_pointer_sample_at {
            return false;
        }
        self.next_pointer_sample_at = now + Duration::from_millis(100);
        let Some(renderer) = &self.renderer else {
            return false;
        };
        let (x, y) = renderer.pointer_vector();
        let body_size = f64::from(CELL_WIDTH.max(CELL_HEIGHT)) * f64::from(self.preferences.scale);
        let gaze_radius = body_size * 1.5;
        self.pointer_nearby = x.hypot(y) <= gaze_radius;
        let direction = self.pointer_nearby.then(|| direction_from_vector(x, y));
        self.state.set_direction(direction)
    }

    fn create_tray(&mut self) -> Result<TrayIcon> {
        let catalog = PetCatalog::scan(&self.pets_root)?;
        let menu = Menu::new();
        let pets_menu = Submenu::new("切换宠物", true);
        for pet in catalog.pets() {
            let selected = pet.manifest.id == self.preferences.selected_pet_id;
            let label = if selected {
                format!("✓ {}", pet.manifest.display_name)
            } else {
                pet.manifest.display_name.clone()
            };
            pets_menu.append(&MenuItem::with_id(
                format!("pet:{}", pet.manifest.id),
                label,
                true,
                None,
            ))?;
        }
        menu.append(&pets_menu)?;
        menu.append(&MenuItem::with_id("state:waving", "打个招呼", true, None))?;
        menu.append(&MenuItem::with_id("state:jumping", "跳一下", true, None))?;
        menu.append(&MenuItem::with_id("state:waiting", "等待", true, None))?;
        menu.append(&MenuItem::with_id("state:running", "执行中", true, None))?;
        menu.append(&MenuItem::with_id("state:review", "完成", true, None))?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&MenuItem::with_id(
            "topmost:toggle",
            if self.preferences.always_on_top {
                "✓ 始终置顶"
            } else {
                "始终置顶"
            },
            true,
            None,
        ))?;
        menu.append(&MenuItem::with_id("show", "显示宠物", true, None))?;
        menu.append(&MenuItem::with_id("hide", "隐藏宠物", true, None))?;
        menu.append(&MenuItem::with_id("quit", "退出 OHM Pet", true, None))?;

        let icon_rgba = self
            .atlas
            .as_ref()
            .ok_or_else(|| anyhow!("atlas unavailable"))?
            .frame_rgba(0, 0);
        let icon = Icon::from_rgba(icon_rgba, CELL_WIDTH, CELL_HEIGHT)
            .map_err(|error| anyhow!("create tray icon: {error}"))?;
        TrayIconBuilder::new()
            .with_tooltip("OHM Pet")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .map_err(|error| anyhow!("create tray: {error}"))
    }

    fn switch_pet(&mut self, id: &str) {
        let Ok(catalog) = PetCatalog::scan(&self.pets_root) else {
            return;
        };
        let Some(pet) = catalog.find(id) else {
            return;
        };
        let Ok(atlas) = pet.load_atlas() else {
            return;
        };
        self.preferences.selected_pet_id = id.to_string();
        self.atlas = Some(atlas);
        self.renderer = None;
        if let (Some(window), Some(atlas)) = (&self.window, &self.atlas) {
            let coordinates = self.state.coordinates();
            self.renderer =
                NativeRenderer::attach(window, atlas, coordinates.row, coordinates.column).ok();
            window.request_redraw();
        }
        if let Some(store) = &self.preferences_store {
            let _ = store.save(&self.preferences);
        }
        match self.create_tray() {
            Ok(tray) => self.tray = Some(tray),
            Err(error) => eprintln!("failed to refresh tray after pet switch: {error:#}"),
        }
    }

    fn handle_menu_events(&mut self, event_loop: &ActiveEventLoop) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id.0.as_str();
            match id {
                "show" => {
                    if let Some(window) = &self.window {
                        window.set_visible(true);
                    }
                }
                "hide" => {
                    if let Some(window) = &self.window {
                        window.set_visible(false);
                    }
                }
                "quit" => event_loop.exit(),
                "topmost:toggle" => {
                    self.preferences.always_on_top = !self.preferences.always_on_top;
                    if let Some(window) = &self.window {
                        window.set_window_level(if self.preferences.always_on_top {
                            WindowLevel::AlwaysOnTop
                        } else {
                            WindowLevel::Normal
                        });
                    }
                    if let Some(store) = &self.preferences_store {
                        let _ = store.save(&self.preferences);
                    }
                    self.tray = self.create_tray().ok();
                }
                "state:waving" => self.state.set_state(
                    AnimationState::Waving,
                    Instant::now(),
                    Some(Duration::from_millis(1800)),
                ),
                "state:jumping" => self.state.set_state(
                    AnimationState::Jumping,
                    Instant::now(),
                    Some(Duration::from_millis(1400)),
                ),
                "state:waiting" => {
                    self.state
                        .set_state(AnimationState::Waiting, Instant::now(), None)
                }
                "state:running" => {
                    self.state
                        .set_state(AnimationState::Running, Instant::now(), None)
                }
                "state:review" => {
                    self.state
                        .set_state(AnimationState::Review, Instant::now(), None)
                }
                _ if id.starts_with("pet:") => self.switch_pet(id.trim_start_matches("pet:")),
                _ => {}
            }
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

impl ApplicationHandler for DesktopPet {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(error) = self.load_selected_pet() {
            eprintln!("OHM Pet could not start: {error:#}");
            event_loop.exit();
            return;
        }

        let scale = self.preferences.scale.clamp(0.42, 1.08) as f64;
        let attributes = Window::default_attributes()
            .with_title("OHM Pet")
            .with_inner_size(LogicalSize::new(
                CELL_WIDTH as f64 * scale,
                CELL_HEIGHT as f64 * scale,
            ))
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_window_level(if self.preferences.always_on_top {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            })
            .with_active(false);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("failed to create window: {error}");
                event_loop.exit();
                return;
            }
        };
        let window_size = window.outer_size();
        let monitors: Vec<MonitorRect> = window
            .available_monitors()
            .map(|monitor| MonitorRect::new(monitor.position(), monitor.size()))
            .collect();
        let primary = window
            .primary_monitor()
            .map(|monitor| MonitorRect::new(monitor.position(), monitor.size()))
            .or_else(|| monitors.first().copied());
        let saved = self
            .preferences
            .window_x
            .zip(self.preferences.window_y)
            .map(|(x, y)| PhysicalPosition::new(x, y));
        if let Some(position) = restored_window_position(saved, window_size, &monitors, primary) {
            window.set_outer_position(position);
            self.preferences.window_x = Some(position.x);
            self.preferences.window_y = Some(position.y);
            if let Some(store) = &self.preferences_store {
                let _ = store.save(&self.preferences);
            }
        }

        let coordinates = self.state.coordinates();
        let renderer = match self.atlas.as_ref().and_then(|atlas| {
            NativeRenderer::attach(&window, atlas, coordinates.row, coordinates.column).ok()
        }) {
            Some(renderer) => renderer,
            None => {
                eprintln!("failed to initialize the native AppKit renderer");
                event_loop.exit();
                return;
            }
        };
        self.renderer = Some(renderer);
        self.window = Some(window.clone());
        match self.create_tray() {
            Ok(tray) => self.tray = Some(tray),
            Err(error) => eprintln!("tray unavailable: {error:#}"),
        }
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.render(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(window) = &self.window {
                    self.press_window_position = window.outer_position().ok();
                    let _ = window.drag_window();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                let moved = self
                    .press_window_position
                    .take()
                    .zip(
                        self.window
                            .as_ref()
                            .and_then(|window| window.outer_position().ok()),
                    )
                    .is_some_and(|(start, end)| {
                        (start.x - end.x).abs() > 2 || (start.y - end.y).abs() > 2
                    });
                self.last_interaction_at = Instant::now();
                self.recent_interactions = self.recent_interactions.saturating_add(1).min(8);
                if !moved {
                    self.state.set_state(
                        AnimationState::Jumping,
                        Instant::now(),
                        Some(Duration::from_millis(1400)),
                    );
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Moved(position) => {
                self.preferences.window_x = Some(position.x);
                self.preferences.window_y = Some(position.y);
                if let Some(store) = &self.preferences_store {
                    let _ = store.save(&self.preferences);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.handle_menu_events(event_loop);
        let now = Instant::now();
        let mut redraw = self.update_pointer_gaze(now) || self.state.tick(now);
        if self.preferences.autonomous
            && now >= self.next_autonomous_at
            && self.state.state() == AnimationState::Idle
        {
            let idle_for = now.saturating_duration_since(self.last_interaction_at);
            if idle_for >= Duration::from_secs(45) {
                self.recent_interactions = 0;
            }
            let decision = self.brain.decide(
                BehaviorContext {
                    idle_for,
                    pointer_nearby: self.pointer_nearby,
                    recent_interactions: self.recent_interactions,
                },
                &mut rand::rng(),
            );
            self.state
                .set_state(decision.state, now, Some(decision.duration));
            self.next_autonomous_at = now + decision.next_thought_in;
            redraw = true;
        }
        if redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        let animation_deadline = if self.preferences.autonomous {
            self.state.next_deadline().min(self.next_autonomous_at)
        } else {
            self.state.next_deadline()
        };
        let deadline = animation_deadline.min(self.next_pointer_sample_at);
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
    }
}

#[derive(Debug, Clone, Copy)]
struct MonitorRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl MonitorRect {
    fn new(position: PhysicalPosition<i32>, size: PhysicalSize<u32>) -> Self {
        Self {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }
    }

    fn intersects_window(self, position: PhysicalPosition<i32>, size: PhysicalSize<u32>) -> bool {
        let right = i64::from(position.x) + i64::from(size.width);
        let bottom = i64::from(position.y) + i64::from(size.height);
        let monitor_right = i64::from(self.x) + i64::from(self.width);
        let monitor_bottom = i64::from(self.y) + i64::from(self.height);
        right > i64::from(self.x) + 16
            && bottom > i64::from(self.y) + 16
            && i64::from(position.x) < monitor_right - 16
            && i64::from(position.y) < monitor_bottom - 16
    }

    fn bottom_right(self, size: PhysicalSize<u32>) -> PhysicalPosition<i32> {
        let margin = 32_i64;
        let x = i64::from(self.x) + i64::from(self.width) - i64::from(size.width) - margin;
        let y = i64::from(self.y) + i64::from(self.height) - i64::from(size.height) - margin;
        PhysicalPosition::new(
            x.max(i64::from(self.x)) as i32,
            y.max(i64::from(self.y)) as i32,
        )
    }
}

fn restored_window_position(
    saved: Option<PhysicalPosition<i32>>,
    window_size: PhysicalSize<u32>,
    monitors: &[MonitorRect],
    primary: Option<MonitorRect>,
) -> Option<PhysicalPosition<i32>> {
    if let Some(saved) = saved {
        if monitors
            .iter()
            .any(|monitor| monitor.intersects_window(saved, window_size))
        {
            return Some(saved);
        }
    }
    primary.map(|monitor| monitor.bottom_right(window_size))
}

fn find_pets_root() -> PathBuf {
    if let Ok(path) = std::env::var("OHM_PET_HOME") {
        return PathBuf::from(path);
    }
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../pets");
    if local.exists() {
        return local;
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            let executable_dir = path.parent()?;
            let bundled = executable_dir.parent()?.join("Resources/pets");
            Some(if bundled.exists() {
                bundled
            } else {
                executable_dir.join("pets")
            })
        })
        .unwrap_or_else(|| PathBuf::from("pets"))
}

fn main() -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = DesktopPet::new(find_pets_root());
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_visible_physical_position() {
        let monitor = MonitorRect {
            x: 0,
            y: 0,
            width: 3024,
            height: 1964,
        };
        let saved = PhysicalPosition::new(1200, 700);
        assert_eq!(
            restored_window_position(
                Some(saved),
                PhysicalSize::new(240, 260),
                &[monitor],
                Some(monitor),
            ),
            Some(saved)
        );
    }

    #[test]
    fn recovers_an_offscreen_position_to_the_primary_display() {
        let monitor = MonitorRect {
            x: 0,
            y: 0,
            width: 3024,
            height: 1964,
        };
        assert_eq!(
            restored_window_position(
                Some(PhysicalPosition::new(4188, 712)),
                PhysicalSize::new(240, 260),
                &[monitor],
                Some(monitor),
            ),
            Some(PhysicalPosition::new(2752, 1672))
        );
    }

    #[test]
    fn preserves_positions_on_secondary_displays() {
        let main = MonitorRect {
            x: 0,
            y: 0,
            width: 3024,
            height: 1964,
        };
        let secondary = MonitorRect {
            x: 3024,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let saved = PhysicalPosition::new(4200, 400);
        assert_eq!(
            restored_window_position(
                Some(saved),
                PhysicalSize::new(240, 260),
                &[main, secondary],
                Some(main),
            ),
            Some(saved)
        );
    }
}
