#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use anyhow::{anyhow, Context, Result};
use directories::BaseDirs;
mod agent_ipc;
mod channel_runtime;
mod channel_settings;
mod channels;
mod integrations;
mod lark_runtime;
mod notifier;
mod task_tracker;

use agent_ipc::{AgentEvent, AgentSignal, UserEvent};
use channel_runtime::spawn_channel_runtime;
use channel_settings::{configure_lark, configure_proxy, configure_telegram};
use channels::{ChannelCommand, ChannelConfigStore};
use integrations::{AgentKind, IntegrationManager};
use lark_runtime::spawn_lark_runtime;
use notifier::{
    notify_task, send_channel_reply, show_channel_notice, test_channel, ChannelKind,
    TaskNotification,
};
use ohm_pet_core::{
    direction_from_vector, AnimationState, Atlas, BehaviorBrain, BehaviorContext, PetCatalog,
    Preferences, PreferencesStore, StateMachine, CELL_HEIGHT, CELL_WIDTH,
};
use task_tracker::TaskTracker;
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
    io::Read,
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

const ACTIVITY_WIDTH: u32 = 340;
const PET_WINDOW_WIDTH: u32 = ACTIVITY_WIDTH + CELL_WIDTH;

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
    tasks: TaskTracker,
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
            tasks: TaskTracker::default(),
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
        let task_lines = self.tasks.display_lines(Instant::now(), 5);
        renderer.set_activity(&task_lines);
        if let Err(error) = renderer.render(atlas, coordinates.row, coordinates.column) {
            eprintln!("render error: {error:#}");
        }
    }

    fn update_pointer_gaze(&mut self, now: Instant) -> bool {
        if now < self.next_pointer_sample_at {
            return false;
        }
        self.next_pointer_sample_at = now + Duration::from_millis(160);
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
        self.create_command_menu()
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

    fn create_command_menu(&self) -> Result<Menu> {
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
        let channel_config = ChannelConfigStore::system()
            .map(|store| store.load())
            .unwrap_or_default();
        let channels_menu = Submenu::new("通讯渠道设置", true);
        channels_menu.append(&MenuItem::with_id(
            "channels:proxy",
            if channel_config.proxy_url.trim().is_empty() {
                "设置网络代理"
            } else {
                "✓ 网络代理已配置"
            },
            true,
            None,
        ))?;
        channels_menu.append(&PredefinedMenuItem::separator())?;
        channels_menu.append(&MenuItem::with_id(
            "channels:telegram",
            if channel_config.telegram.ready() {
                "✓ Telegram Bot"
            } else {
                "设置 Telegram Bot"
            },
            true,
            None,
        ))?;
        channels_menu.append(&MenuItem::with_id(
            "channels:test-telegram",
            "发送 Telegram 测试消息",
            channel_config.telegram.ready(),
            None,
        ))?;
        channels_menu.append(&PredefinedMenuItem::separator())?;
        channels_menu.append(&MenuItem::with_id(
            "channels:lark",
            if channel_config.lark.ready() {
                "✓ 飞书 / Lark Bot（实验性·未实测）"
            } else {
                "设置飞书 / Lark Bot（实验性·未实测）"
            },
            true,
            None,
        ))?;
        channels_menu.append(&MenuItem::with_id(
            "channels:test-lark",
            "发送飞书 / Lark 测试消息（未实测）",
            channel_config.lark.ready(),
            None,
        ))?;
        if let Some(store) = ChannelConfigStore::system() {
            channels_menu.append(&MenuItem::with_id(
                "channels:show-path",
                format!("配置文件：{}", store.path().display()),
                false,
                None,
            ))?;
        }
        menu.append(&channels_menu)?;
        let task_lines = self.tasks.display_lines(Instant::now(), 5);
        if !task_lines.is_empty() {
            let tasks_menu = Submenu::new(format!("活跃任务（{}）", task_lines.len()), true);
            for (index, line) in task_lines.into_iter().enumerate() {
                tasks_menu.append(&MenuItem::with_id(
                    format!("task:status:{index}"),
                    line,
                    false,
                    None,
                ))?;
            }
            menu.append(&tasks_menu)?;
        }
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
        Ok(menu)
    }

    fn create_tray_icon(&self) -> Result<Icon> {
        let icon_rgba = self
            .atlas
            .as_ref()
            .ok_or_else(|| anyhow!("atlas unavailable"))?
            .frame_rgba(0, 0);
        Icon::from_rgba(icon_rgba, CELL_WIDTH, CELL_HEIGHT)
            .map_err(|error| anyhow!("create tray icon: {error}"))
    }

    fn create_tray(&self) -> Result<TrayIcon> {
        TrayIconBuilder::new()
            .with_tooltip("OHM Pet")
            .with_icon(self.create_tray_icon()?)
            .with_menu(Box::new(self.create_command_menu()?))
            .build()
            .map_err(|error| anyhow!("create tray: {error}"))
    }

    fn refresh_tray(&mut self) {
        let menu = match self.create_command_menu() {
            Ok(menu) => menu,
            Err(error) => {
                eprintln!("failed to refresh tray menu: {error:#}");
                return;
            }
        };
        let icon = match self.create_tray_icon() {
            Ok(icon) => icon,
            Err(error) => {
                eprintln!("failed to refresh tray icon: {error:#}");
                return;
            }
        };
        if let Some(tray) = self.tray.as_mut() {
            tray.set_menu(Some(Box::new(menu)));
            if let Err(error) = tray.set_icon(Some(icon)) {
                eprintln!("failed to update tray icon: {error}");
            }
            return;
        }
        match TrayIconBuilder::new()
            .with_tooltip("OHM Pet")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
        {
            Ok(tray) => self.tray = Some(tray),
            Err(error) => eprintln!("failed to recreate tray: {error}"),
        }
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
        self.refresh_tray();
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
        self.refresh_tray();
    }

    fn handle_channel_command(&self, command: ChannelCommand) {
        let command_name = command
            .text
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .split('@')
            .next()
            .unwrap_or_default();
        let now = Instant::now();
        let response = match command_name {
            "/tasks" => {
                let lines = self.tasks.display_lines(now, 20);
                if lines.is_empty() {
                    "OHM Pet: no active tasks.".to_owned()
                } else {
                    format!("OHM Pet active tasks:\n{}", lines.join("\n"))
                }
            }
            "/status" => {
                let state = match self.tasks.animation_state() {
                    AnimationState::Waiting => "waiting for input",
                    AnimationState::Running => "working",
                    AnimationState::Failed => "failed",
                    AnimationState::Review => "ready",
                    _ => "idle",
                };
                let lines = self.tasks.display_lines(now, 5);
                if lines.is_empty() {
                    format!("OHM Pet status: {state}.")
                } else {
                    format!("OHM Pet status: {state}.\n{}", lines.join("\n"))
                }
            }
            "/help" | "/start" => [
                "OHM Pet remote commands:",
                "/status - current aggregate status",
                "/tasks - active task list",
                "/help - command help",
                "",
                "Task mutation commands are not enabled yet.",
            ]
            .join("\n"),
            _ => "Unknown command. Use /help.".to_owned(),
        };
        send_channel_reply(&command, response);
    }

    fn apply_agent_signal(&mut self, signal: AgentSignal) {
        self.roam = None;
        let now = Instant::now();
        let previous_state = self.tasks.animation_state();
        let should_notify = signal.source != "pi" || signal.task_id.as_deref() == Some("current");
        let update = self.tasks.apply(&signal, now);
        let aggregate = self.tasks.animation_state();
        if aggregate != previous_state {
            self.state.set_state(aggregate, now, None);
        }
        if should_notify {
            if let Some(task) = update.finished {
                let elapsed = task.elapsed(now);
                notify_task(TaskNotification {
                    source: task.source,
                    title: task.title,
                    event: task.event,
                    elapsed,
                });
            }
        }
        if update.changed {
            self.refresh_tray();
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
        self.refresh_tray();
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
                    self.refresh_tray();
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
                "channels:proxy" => {
                    match configure_proxy() {
                        Ok(true) => show_channel_notice(
                            "Network proxy settings saved",
                            "Telegram and API requests will use the new proxy automatically.",
                        ),
                        Ok(false) => {}
                        Err(error) => {
                            show_channel_notice("Network proxy settings failed", &error.to_string())
                        }
                    }
                    self.refresh_tray();
                }
                "channels:telegram" => {
                    match configure_telegram() {
                        Ok(true) => show_channel_notice(
                            "Telegram settings saved",
                            "The new channel configuration will be applied automatically.",
                        ),
                        Ok(false) => {}
                        Err(error) => {
                            eprintln!("Telegram settings error: {error:#}");
                            show_channel_notice("Telegram settings failed", &error.to_string());
                        }
                    }
                    self.refresh_tray();
                }
                "channels:test-telegram" => test_channel(ChannelKind::Telegram),
                "channels:lark" => {
                    match configure_lark() {
                        Ok(true) => show_channel_notice(
                            "Feishu / Lark settings saved",
                            "The new channel configuration will be applied automatically.",
                        ),
                        Ok(false) => {}
                        Err(error) => {
                            eprintln!("Lark settings error: {error:#}");
                            show_channel_notice(
                                "Feishu / Lark settings failed",
                                &error.to_string(),
                            );
                        }
                    }
                    self.refresh_tray();
                }
                "channels:test-lark" => test_channel(ChannelKind::Lark),
                "integration:test" => self.apply_agent_signal(AgentSignal {
                    source: "tray".into(),
                    event: AgentEvent::Completed,
                    title: Some("Integration test".into()),
                    session_id: Some("tray".into()),
                    task_id: Some("test".into()),
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
                    self.refresh_tray();
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
                PET_WINDOW_WIDTH as f64 * scale,
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
            UserEvent::ChannelCommand(command) => self.handle_channel_command(command),
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
        if self.tasks.prune(now) {
            self.state
                .set_state(self.tasks.animation_state(), now, None);
            self.refresh_tray();
            redraw = true;
        }
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
        let animation_deadline =
            if self.preferences.autonomous && self.state.state() == AnimationState::Idle {
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
    payload_stdin: bool,
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
    let mut session_id = None;
    let mut task_id = None;
    let mut codex_payload = None;
    let mut payload_stdin = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--source" => source = arguments.next(),
            "--event" => event = arguments.next(),
            "--title" => title = arguments.next(),
            "--session-id" => session_id = arguments.next(),
            "--task-id" => task_id = arguments.next(),
            "--payload-stdin" => payload_stdin = true,
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
            session_id,
            task_id,
        },
        codex_payload,
        payload_stdin,
    }))
}

fn enrich_signal_from_payload(signal: &mut AgentSignal, payload: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return;
    };
    let string = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
            .map(str::to_owned)
    };
    if signal.session_id.is_none() {
        signal.session_id = string(&["session_id", "sessionId", "thread-id", "thread_id"]);
    }
    if signal.task_id.is_none() {
        signal.task_id = string(&["task_id", "taskId", "turn-id", "turn_id"]);
    }
    if signal.title.is_none() {
        signal.title = string(&["prompt", "title"]);
        if signal.title.is_none() {
            signal.title = value
                .get("input-messages")
                .or_else(|| value.get("input_messages"))
                .and_then(serde_json::Value::as_array)
                .and_then(|messages| messages.first())
                .and_then(|message| {
                    message
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| message.get("content")?.as_str().map(str::to_owned))
                });
        }
    }
}

fn main() -> Result<()> {
    if let Some(mut command) = parse_signal_command(std::env::args().skip(1))? {
        let stdin_payload = if command.payload_stdin {
            let mut payload = String::new();
            std::io::stdin().read_to_string(&mut payload)?;
            (!payload.trim().is_empty()).then_some(payload)
        } else {
            None
        };
        if let Some(payload) = stdin_payload
            .as_deref()
            .or(command.codex_payload.as_deref())
        {
            enrich_signal_from_payload(&mut command.signal, payload);
        }
        agent_ipc::send_signal(&command.signal)?;
        if command.signal.source == "codex" {
            if let Some(payload) = command.codex_payload {
                IntegrationManager::system()?.forward_previous_codex_notify(&payload)?;
            }
        }
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    if let Err(error) = notify_rust::set_application("works.earendil.ohm-pet") {
        eprintln!("Native notification application setup failed: {error}");
    }

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    if let Err(error) = agent_ipc::spawn_signal_listener(event_loop.create_proxy()) {
        eprintln!("Agent event listener unavailable: {error:#}");
    }
    spawn_channel_runtime(event_loop.create_proxy());
    spawn_lark_runtime(event_loop.create_proxy());
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
