#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use anyhow::{anyhow, Context, Result};
use directories::BaseDirs;
mod agent_ipc;
mod integrations;

use agent_ipc::{AgentEvent, AgentSignal, UserEvent};
use integrations::{AgentKind, IntegrationManager};
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

use rand::Rng;
#[cfg(debug_assertions)]
use std::path::Path;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tray_icon::{
    menu::{ContextMenu, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    Icon, TrayIcon, TrayIconBuilder,
};
#[cfg(target_os = "windows")]
use winit::platform::windows::WindowAttributesExtWindows;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::{Window, WindowId, WindowLevel},
};

struct RoamPlan {
    start_x: i32,
    target_x: i32,
    y: i32,
    land_at: Option<Instant>,
    next_step_at: Instant,
}

struct DesktopPet {
    window: Option<Arc<Window>>,
    renderer: Option<NativeRenderer>,
    atlas: Option<Atlas>,
    state: StateMachine,
    preferences: Preferences,
    preferences_store: Option<PreferencesStore>,
    pet_roots: Vec<PathBuf>,
    next_autonomous_at: Instant,
    next_pointer_sample_at: Instant,
    tray: Option<TrayIcon>,
    press_window_position: Option<PhysicalPosition<i32>>,
    brain: BehaviorBrain,
    last_interaction_at: Instant,
    recent_interactions: u32,
    pointer_nearby: bool,
    roam: Option<RoamPlan>,
    next_roam_at: Instant,
}

impl DesktopPet {
    fn new(pet_roots: Vec<PathBuf>) -> Self {
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
            pet_roots,
            next_autonomous_at: now + Duration::from_secs(14),
            next_pointer_sample_at: now,
            tray: None,
            press_window_position: None,
            brain: BehaviorBrain::default(),
            last_interaction_at: now,
            recent_interactions: 0,
            pointer_nearby: false,
            roam: None,
            next_roam_at: now + Duration::from_secs(18),
        }
    }

    fn load_selected_pet(&mut self) -> Result<()> {
        let catalog = PetCatalog::scan_many(&self.pet_roots).context("scan pet catalogs")?;
        let pet = catalog
            .find(&self.preferences.selected_pet_id)
            .or_else(|| catalog.pets().first())
            .ok_or_else(|| anyhow!("no valid pets found in configured pet directories"))?;
        self.preferences.selected_pet_id = pet.manifest.id.clone();
        let costumes = self
            .preferences
            .selected_costumes
            .get(&pet.manifest.id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        self.atlas = Some(
            pet.load_atlas_with_costumes(costumes)
                .context("load selected pet atlas")?,
        );
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

    fn update_roaming(&mut self, now: Instant) -> bool {
        if let Some(mut roam) = self.roam.take() {
            let Some(window) = self.window.clone() else {
                return false;
            };
            if let Some(land_at) = roam.land_at {
                if now < land_at {
                    self.roam = Some(roam);
                    return false;
                }
                window.set_outer_position(PhysicalPosition::new(roam.start_x, roam.y));
                self.state.set_state(
                    if roam.target_x >= roam.start_x {
                        AnimationState::RunningRight
                    } else {
                        AnimationState::RunningLeft
                    },
                    now,
                    None,
                );
                roam.land_at = None;
                roam.next_step_at = now;
                self.roam = Some(roam);
                return true;
            }
            if now < roam.next_step_at {
                self.roam = Some(roam);
                return false;
            }
            let current_x = window
                .outer_position()
                .map_or(roam.start_x, |position| position.x);
            let difference = roam.target_x - current_x;
            if difference.abs() <= 4 {
                window.set_outer_position(PhysicalPosition::new(roam.target_x, roam.y));
                self.preferences.window_x = Some(roam.target_x);
                self.preferences.window_y = Some(roam.y);
                if let Some(store) = &self.preferences_store {
                    let _ = store.save(&self.preferences);
                }
                self.state.set_state(AnimationState::Idle, now, None);
                self.next_roam_at = now + Duration::from_secs(rand::rng().random_range(18..46));
                self.next_autonomous_at = now + Duration::from_secs(10);
                return true;
            }
            let next_x = current_x + difference.signum() * 4;
            window.set_outer_position(PhysicalPosition::new(next_x, roam.y));
            roam.next_step_at = now + Duration::from_millis(40);
            self.roam = Some(roam);
            return true;
        }

        if !self.preferences.autonomous
            || now < self.next_roam_at
            || self.pointer_nearby
            || self.state.state() != AnimationState::Idle
        {
            return false;
        }
        let Some(window) = self.window.clone() else {
            return false;
        };
        let Some((surface_left, surface_right, surface_top)) = self
            .renderer
            .as_ref()
            .and_then(NativeRenderer::walkable_surface)
        else {
            self.next_roam_at = now + Duration::from_secs(12);
            return false;
        };
        let size = window.outer_size();
        let min_x = surface_left;
        let max_x = surface_right - size.width as i32;
        if max_x - min_x < 96 {
            self.next_roam_at = now + Duration::from_secs(12);
            return false;
        }
        let current_x = window
            .outer_position()
            .map_or(min_x, |position| position.x.clamp(min_x, max_x));
        let mut rng = rand::rng();
        let mut target_x = rng.random_range(min_x..=max_x);
        if (target_x - current_x).abs() < 72 {
            target_x = if current_x - min_x > max_x - current_x {
                min_x
            } else {
                max_x
            };
        }
        let y = surface_top - size.height as i32 + 4;
        self.state.set_state(AnimationState::Jumping, now, None);
        self.roam = Some(RoamPlan {
            start_x: current_x,
            target_x,
            y,
            land_at: Some(now + Duration::from_millis(520)),
            next_step_at: now + Duration::from_millis(520),
        });
        true
    }

    fn create_pet_context_menu(&self) -> Result<Menu> {
        let menu = Menu::new();
        let catalog = PetCatalog::scan_many(&self.pet_roots)?;
        let Some(pet) = catalog.find(&self.preferences.selected_pet_id) else {
            return Ok(menu);
        };
        let selected = self
            .preferences
            .selected_costumes
            .get(&pet.manifest.id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if pet.costumes.is_empty() {
            menu.append(&MenuItem::new("此宠物没有可用换装", false, None))?;
        } else {
            menu.append(&MenuItem::with_id(
                "costume:clear",
                if selected.is_empty() {
                    "✓ 默认装扮"
                } else {
                    "默认装扮"
                },
                true,
                None,
            ))?;
            for costume in &pet.costumes {
                menu.append(&MenuItem::with_id(
                    format!("costume:{}", costume.id),
                    format!(
                        "{}{}：{}",
                        if selected.contains(&costume.id) {
                            "✓ "
                        } else {
                            ""
                        },
                        costume.category,
                        costume.name
                    ),
                    true,
                    None,
                ))?;
            }
        }
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&MenuItem::with_id(
            "state:roam",
            "沿前台窗口走一走",
            true,
            None,
        ))?;
        Ok(menu)
    }

    fn show_pet_context_menu(&self) {
        let (Some(window), Ok(menu)) = (&self.window, self.create_pet_context_menu()) else {
            return;
        };
        let Ok(handle) = window.window_handle() else {
            return;
        };
        match handle.as_raw() {
            #[cfg(target_os = "windows")]
            RawWindowHandle::Win32(handle) => unsafe {
                let _ = menu.show_context_menu_for_hwnd(handle.hwnd.get(), None);
            },
            #[cfg(target_os = "macos")]
            RawWindowHandle::AppKit(handle) => unsafe {
                let _ = menu.show_context_menu_for_nsview(handle.ns_view.as_ptr().cast(), None);
            },
            _ => {}
        }
    }

    fn create_tray(&mut self) -> Result<TrayIcon> {
        let catalog = PetCatalog::scan_many(&self.pet_roots)?;
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
        menu.append(&MenuItem::with_id(
            "pets:refresh",
            "刷新宠物目录",
            true,
            None,
        ))?;
        if let Some(pet) = catalog.find(&self.preferences.selected_pet_id) {
            if !pet.costumes.is_empty() {
                let costume_menu = Submenu::new("宠物换装", true);
                let selected = self
                    .preferences
                    .selected_costumes
                    .get(&pet.manifest.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                costume_menu.append(&MenuItem::with_id(
                    "costume:clear",
                    if selected.is_empty() {
                        "✓ 默认装扮"
                    } else {
                        "默认装扮"
                    },
                    true,
                    None,
                ))?;
                for costume in &pet.costumes {
                    let checked = selected.contains(&costume.id);
                    costume_menu.append(&MenuItem::with_id(
                        format!("costume:{}", costume.id),
                        format!(
                            "{}{}：{}",
                            if checked { "✓ " } else { "" },
                            costume.category,
                            costume.name
                        ),
                        true,
                        None,
                    ))?;
                }
                menu.append(&costume_menu)?;
            }
        }
        let integration_status = IntegrationManager::system()
            .map(|manager| manager.status())
            .unwrap_or_default();
        let integrations_menu = Submenu::new("Agent 集成", true);
        integrations_menu.append(&MenuItem::with_id(
            "integration:claude:install",
            if integration_status.claude {
                "✓ Claude Code 已接入（点击更新）"
            } else {
                "接入 Claude Code"
            },
            true,
            None,
        ))?;
        integrations_menu.append(&MenuItem::with_id(
            "integration:codex:install",
            if integration_status.codex {
                "✓ Codex 已接入（点击更新）"
            } else {
                "接入 Codex"
            },
            true,
            None,
        ))?;
        integrations_menu.append(&MenuItem::with_id(
            "integration:pi:install",
            if integration_status.pi {
                "✓ Pi Agent 已接入（点击更新）"
            } else {
                "接入 Pi Agent"
            },
            true,
            None,
        ))?;
        integrations_menu.append(&PredefinedMenuItem::separator())?;
        integrations_menu.append(&MenuItem::with_id(
            "integration:test",
            "测试 Agent 动画",
            true,
            None,
        ))?;
        integrations_menu.append(&MenuItem::with_id(
            "integration:remove-all",
            "移除全部 Agent 集成",
            integration_status.claude || integration_status.codex || integration_status.pi,
            None,
        ))?;
        menu.append(&integrations_menu)?;
        menu.append(&MenuItem::with_id("state:waving", "打个招呼", true, None))?;
        menu.append(&MenuItem::with_id("state:jumping", "跳一下", true, None))?;
        menu.append(&MenuItem::with_id("state:waiting", "等待", true, None))?;
        menu.append(&MenuItem::with_id("state:running", "执行中", true, None))?;
        menu.append(&MenuItem::with_id("state:review", "完成", true, None))?;
        menu.append(&MenuItem::with_id(
            "state:roam",
            "沿前台窗口走一走",
            true,
            None,
        ))?;
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
        let Ok(catalog) = PetCatalog::scan_many(&self.pet_roots) else {
            return;
        };
        let Some(pet) = catalog.find(id) else {
            return;
        };
        let costumes = self
            .preferences
            .selected_costumes
            .get(id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let Ok(atlas) = pet.load_atlas_with_costumes(costumes) else {
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

    fn toggle_costume(&mut self, costume_id: Option<&str>) {
        let Ok(catalog) = PetCatalog::scan_many(&self.pet_roots) else {
            return;
        };
        let Some(pet) = catalog.find(&self.preferences.selected_pet_id) else {
            return;
        };
        let mut selected = self
            .preferences
            .selected_costumes
            .get(&pet.manifest.id)
            .cloned()
            .unwrap_or_default();
        match costume_id {
            None => selected.clear(),
            Some(id) if selected.iter().any(|value| value == id) => {
                selected.retain(|value| value != id);
            }
            Some(id) => {
                let Some(costume) = pet.costumes.iter().find(|costume| costume.id == id) else {
                    return;
                };
                let same_category: Vec<&str> = pet
                    .costumes
                    .iter()
                    .filter(|option| option.category == costume.category)
                    .map(|option| option.id.as_str())
                    .collect();
                selected.retain(|value| !same_category.contains(&value.as_str()));
                selected.push(id.to_owned());
            }
        }
        let Ok(atlas) = pet.load_atlas_with_costumes(&selected) else {
            return;
        };
        if selected.is_empty() {
            self.preferences.selected_costumes.remove(&pet.manifest.id);
        } else {
            self.preferences
                .selected_costumes
                .insert(pet.manifest.id.clone(), selected);
        }
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
        self.tray = self.create_tray().ok();
    }

    fn apply_agent_signal(&mut self, signal: AgentSignal) {
        self.roam = None;
        let now = Instant::now();
        match signal.event {
            AgentEvent::Working => self.state.set_state(AnimationState::Running, now, None),
            AgentEvent::Waiting => self.state.set_state(AnimationState::Waiting, now, None),
            AgentEvent::Completed => self.state.set_state(
                AnimationState::Review,
                now,
                Some(Duration::from_millis(4200)),
            ),
            AgentEvent::Failed => self.state.set_state(
                AnimationState::Failed,
                now,
                Some(Duration::from_millis(5200)),
            ),
            AgentEvent::Idle => self.state.set_state(AnimationState::Idle, now, None),
        }
        if let Some(window) = &self.window {
            window.set_visible(true);
            window.request_redraw();
        }
    }

    fn run_integration_action(&mut self, action: impl FnOnce(&IntegrationManager) -> Result<()>) {
        let result = IntegrationManager::system().and_then(|manager| action(&manager));
        let now = Instant::now();
        if result.is_ok() {
            self.state.set_state(
                AnimationState::Review,
                now,
                Some(Duration::from_millis(2200)),
            );
        } else {
            if let Err(error) = result {
                eprintln!("Agent integration error: {error:#}");
            }
            self.state.set_state(
                AnimationState::Failed,
                now,
                Some(Duration::from_millis(4200)),
            );
        }
        self.tray = self.create_tray().ok();
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
                "pets:refresh" => {
                    self.pet_roots = discover_pet_roots();
                    self.tray = self.create_tray().ok();
                }
                "integration:claude:install" => {
                    self.run_integration_action(|manager| manager.install(AgentKind::Claude));
                }
                "integration:codex:install" => {
                    self.run_integration_action(|manager| manager.install(AgentKind::Codex));
                }
                "integration:pi:install" => {
                    self.run_integration_action(|manager| manager.install(AgentKind::Pi));
                }
                "integration:remove-all" => {
                    self.run_integration_action(|manager| {
                        manager.remove(AgentKind::Claude)?;
                        manager.remove(AgentKind::Codex)?;
                        manager.remove(AgentKind::Pi)
                    });
                }
                "integration:test" => self.apply_agent_signal(AgentSignal {
                    source: "tray".into(),
                    event: AgentEvent::Completed,
                    title: Some("Integration test".into()),
                }),
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
                "state:roam" => {
                    self.roam = None;
                    self.state
                        .set_state(AnimationState::Idle, Instant::now(), None);
                    self.next_roam_at = Instant::now();
                }
                "costume:clear" => self.toggle_costume(None),
                _ if id.starts_with("costume:") => {
                    self.toggle_costume(Some(id.trim_start_matches("costume:")))
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

impl ApplicationHandler<UserEvent> for DesktopPet {
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
        #[cfg(target_os = "windows")]
        let attributes = attributes.with_skip_taskbar(true);
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

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::AgentSignal(signal) => self.apply_agent_signal(signal),
        }
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
                button: MouseButton::Right,
                ..
            } => self.show_pet_context_menu(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(window) = self.window.clone() {
                    self.roam = None;
                    self.next_roam_at = Instant::now() + Duration::from_secs(18);
                    self.press_window_position = window.outer_position().ok();
                    self.state
                        .set_state(AnimationState::Jumping, Instant::now(), None);
                    self.render();
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
                if moved {
                    self.state
                        .set_state(AnimationState::Idle, Instant::now(), None);
                } else {
                    self.state.set_state(
                        AnimationState::Jumping,
                        Instant::now(),
                        Some(Duration::from_millis(1400)),
                    );
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::Moved(position) if self.roam.is_none() => {
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
        let mut redraw = self.update_pointer_gaze(now);
        redraw |= self.update_roaming(now);
        redraw |= self.state.tick(now);
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
        let roam_deadline = self.roam.as_ref().map_or_else(
            || {
                if self.preferences.autonomous
                    && !self.pointer_nearby
                    && self.state.state() == AnimationState::Idle
                {
                    self.next_roam_at
                } else {
                    now + Duration::from_secs(24 * 60 * 60)
                }
            },
            |roam| roam.land_at.unwrap_or(roam.next_step_at),
        );
        let deadline = animation_deadline
            .min(self.next_pointer_sample_at)
            .min(roam_deadline);
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

fn discover_pet_roots() -> Vec<PathBuf> {
    use std::collections::HashSet;

    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |path: PathBuf| {
        if path.is_dir() && seen.insert(path.clone()) {
            roots.push(path);
        }
    };

    if let Ok(path) = std::env::var("OHM_PET_HOME") {
        add(PathBuf::from(path));
    }

    #[cfg(debug_assertions)]
    add(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/default-pets"));

    #[cfg(target_os = "macos")]
    let mut bundled_fallback = None;
    if let Ok(executable) = std::env::current_exe() {
        if let Some(executable_dir) = executable.parent() {
            #[cfg(target_os = "windows")]
            add(executable_dir.join("pets"));

            #[cfg(target_os = "macos")]
            if let Some(contents_dir) = executable_dir.parent() {
                if let Some(application_dir) = contents_dir.parent().and_then(|path| path.parent())
                {
                    add(application_dir.join("pets"));
                }
                bundled_fallback = Some(contents_dir.join("Resources/pets"));
            }

            add(executable_dir.join("pets"));
        }
    }

    if let Some(base_dirs) = BaseDirs::new() {
        let home = base_dirs.home_dir();
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        add(codex_home.join("pets"));

        let claude_home = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"));
        add(claude_home.join("pets"));
        add(base_dirs.config_dir().join("Claude/pets"));
    }

    #[cfg(target_os = "macos")]
    if let Some(bundled) = bundled_fallback {
        add(bundled);
    }
    roots
}

struct SignalCommand {
    signal: AgentSignal,
    codex_payload: Option<String>,
}

fn parse_signal_command(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Option<SignalCommand>> {
    let mut arguments = arguments.into_iter();
    if arguments.next().as_deref() != Some("signal") {
        return Ok(None);
    }
    let mut source = None;
    let mut event = None;
    let mut title = None;
    let mut codex_payload = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--source" => source = arguments.next(),
            "--event" => event = arguments.next(),
            "--title" => title = arguments.next(),
            "--integration" => {
                let _ = arguments.next();
            }
            value if value.starts_with('{') => codex_payload = Some(value.to_owned()),
            value => return Err(anyhow!("unsupported signal argument: {value}")),
        }
    }
    let source = source.ok_or_else(|| anyhow!("signal requires --source"))?;
    let event = event
        .ok_or_else(|| anyhow!("signal requires --event"))?
        .parse::<AgentEvent>()?;
    Ok(Some(SignalCommand {
        signal: AgentSignal {
            source,
            event,
            title,
        },
        codex_payload,
    }))
}

fn main() -> Result<()> {
    if let Some(command) = parse_signal_command(std::env::args().skip(1))? {
        agent_ipc::send_signal(&command.signal)?;
        if command.signal.source == "codex" {
            if let Some(payload) = command.codex_payload {
                IntegrationManager::system()?.forward_previous_codex_notify(&payload)?;
            }
        }
        return Ok(());
    }

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    if let Err(error) = agent_ipc::spawn_signal_listener(event_loop.create_proxy()) {
        eprintln!("Agent event listener unavailable: {error:#}");
    }
    let mut app = DesktopPet::new(discover_pet_roots());
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agent_signal_command_and_codex_payload() {
        let command = parse_signal_command([
            "signal".into(),
            "--source".into(),
            "codex".into(),
            "--event".into(),
            "completed".into(),
            "--integration".into(),
            "ohm-pet".into(),
            r#"{"type":"agent-turn-complete"}"#.into(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(command.signal.source, "codex");
        assert_eq!(command.signal.event, AgentEvent::Completed);
        assert!(command
            .codex_payload
            .unwrap()
            .contains("agent-turn-complete"));
    }

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
