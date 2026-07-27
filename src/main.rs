mod audio;
mod config;
mod game;
mod input;
mod persistence;
mod physics;
mod render;
mod terrain;
mod ui;

use audio::AudioSystem;
use game::{Game, GameScreen};
use input::InputState;
use persistence::{SaveData, ScreenMode, VisualStyle};
use render::Renderer;
use std::{sync::Arc, time::Instant};
use ui::UiAction;
use winit::{
    dpi::PhysicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::PhysicalKey,
    window::{Fullscreen, Window, WindowBuilder},
};

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,wgpu_core=error"),
    )
    .init();
    if let Err(error) = pollster::block_on(run()) {
        eprintln!("RIDGELINE could not start: {error}");
    }
}

async fn run() -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|error| error.to_string())?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title(config::WINDOW_TITLE)
            .with_inner_size(PhysicalSize::new(
                config::INITIAL_WIDTH,
                config::INITIAL_HEIGHT,
            ))
            .with_min_inner_size(PhysicalSize::new(960, 600))
            .build(&event_loop)
            .map_err(|error| error.to_string())?,
    );
    let mut app = App::new(window.clone()).await?;

    event_loop
        .run(move |event, target| {
            target.set_control_flow(ControlFlow::Poll);
            match event {
                Event::WindowEvent { window_id, event } if window_id == window.id() => {
                    let response = app.egui_state.on_window_event(&window, &event);
                    if response.repaint {
                        window.request_redraw();
                    }
                    match event {
                        WindowEvent::CloseRequested => {
                            app.save.store();
                            target.exit();
                        }
                        WindowEvent::Resized(size) => app.renderer.resize(size),
                        WindowEvent::ScaleFactorChanged { .. } => {
                            app.renderer.resize(window.inner_size())
                        }
                        WindowEvent::KeyboardInput { event, .. } => {
                            if let PhysicalKey::Code(code) = event.physical_key {
                                app.input.keyboard(code, event.state, event.repeat);
                            }
                        }
                        WindowEvent::RedrawRequested if app.frame(&window) => {
                            app.save.store();
                            target.exit();
                        }
                        WindowEvent::RedrawRequested => {}
                        _ => {}
                    }
                }
                Event::AboutToWait => window.request_redraw(),
                _ => {}
            }
        })
        .map_err(|error| error.to_string())
}

struct App {
    renderer: Renderer,
    game: Game,
    input: InputState,
    audio: AudioSystem,
    ui_assets: ui::UiAssets,
    save: SaveData,
    egui_context: egui::Context,
    egui_state: egui_winit::State,
    last_frame: Instant,
    accumulator: f32,
    autoplay: bool,
    freeze_splash: bool,
    freeze_simulation: bool,
    progression_save_timer: f32,
}

impl App {
    async fn new(window: Arc<Window>) -> Result<Self, String> {
        let arguments: Vec<_> = std::env::args().collect();
        let preview_seed = arguments.iter().find_map(|argument| {
            argument
                .strip_prefix("--seed=")
                .and_then(|seed| u64::from_str_radix(seed.trim_start_matches("0x"), 16).ok())
        });
        let factory_preview = arguments
            .iter()
            .any(|argument| argument == "--factory-preview");
        let ultrawide = arguments.iter().any(|argument| argument == "--ultrawide");
        let mut save = if factory_preview {
            SaveData::default()
        } else {
            SaveData::load()
        };
        let vaporwave_preview = arguments
            .iter()
            .any(|argument| argument == "--vaporwave-preview");
        let dark_preview = arguments
            .iter()
            .any(|argument| argument == "--dark-preview");
        if vaporwave_preview {
            save.settings.visual_style = VisualStyle::Vaporwave;
        } else if dark_preview {
            save.settings.visual_style = VisualStyle::Dark;
        }
        if ultrawide {
            save.settings.resolution = [2560, 1080];
            save.settings.screen_mode = ScreenMode::Windowed;
            if !factory_preview {
                save.store();
            }
        }
        apply_video_settings(&window, &save);
        let renderer = Renderer::new(window.clone()).await?;
        let audio = AudioSystem::new(&save.settings);
        let egui_context = egui::Context::default();
        ui::configure_style(&egui_context);
        let egui_state = egui_winit::State::new(
            egui_context.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
        );
        let party_preview = arguments
            .iter()
            .any(|argument| argument == "--party-preview");
        let tear_preview = arguments
            .iter()
            .any(|argument| argument == "--tear-preview");
        let settings_preview = arguments
            .iter()
            .any(|argument| argument == "--settings-preview");
        let achievements_preview = arguments
            .iter()
            .any(|argument| argument == "--achievements-preview");
        let tutorial_preview = arguments
            .iter()
            .any(|argument| argument == "--tutorial-preview");
        let splash_preview = arguments
            .iter()
            .any(|argument| argument == "--splash-preview");
        let menu_preview = arguments
            .iter()
            .any(|argument| argument == "--menu-preview");
        let pause_preview = arguments
            .iter()
            .any(|argument| argument == "--pause-preview");
        let game_over_preview = arguments
            .iter()
            .any(|argument| argument == "--game-over-preview");
        let autoplay = party_preview
            || vaporwave_preview
            || dark_preview
            || arguments.iter().any(|argument| argument == "--autoplay");
        let mut game =
            Game::with_style(save.settings.terrain_intensity, save.settings.visual_style);
        game.apply_trail_settings(save.settings.trail_style, save.settings.trail_deformation);
        if autoplay {
            if let Some(seed) = preview_seed {
                game.start_seed_for_preview(seed, save.settings.terrain_intensity);
            } else {
                game.start_new_seed(save.settings.terrain_intensity);
            }
        }
        if tear_preview {
            if let Some(seed) = preview_seed {
                game.start_seed_for_preview(seed, save.settings.terrain_intensity);
            } else {
                game.start_new_seed(save.settings.terrain_intensity);
            }
            game.enable_tear_preview();
        } else if party_preview {
            game.enable_party_preview();
        } else if settings_preview {
            game.screen = GameScreen::Settings;
        } else if achievements_preview {
            game.screen = GameScreen::Achievements;
        } else if tutorial_preview {
            game.start_new_seed(save.settings.terrain_intensity);
            game.screen = GameScreen::Tutorial;
        } else if pause_preview {
            game.start_new_seed(save.settings.terrain_intensity);
            game.score = 8_600;
            game.distance = 1_284.0;
            game.streak = 9;
            game.screen = GameScreen::Paused;
        } else if game_over_preview {
            game.start_new_seed(save.settings.terrain_intensity);
            game.score = 14_200;
            game.distance = 2_467.0;
            game.streak = 12;
            game.screen = GameScreen::GameOver;
        } else if menu_preview {
            game.screen = GameScreen::Menu;
        } else if splash_preview {
            game.screen = GameScreen::Splash;
        }
        Ok(Self {
            renderer,
            game,
            input: InputState::new(),
            audio,
            ui_assets: ui::UiAssets::default(),
            save,
            egui_context,
            egui_state,
            last_frame: Instant::now(),
            accumulator: 0.0,
            autoplay,
            freeze_splash: splash_preview,
            freeze_simulation: tear_preview,
            progression_save_timer: 0.0,
        })
    }

    /// Advances simulation on a 120 Hz clock, while rendering interpolated transforms at the
    /// display refresh rate. The short frame clamp prevents a debugger pause from exploding the
    /// physics accumulator.
    fn frame(&mut self, window: &Window) -> bool {
        let now = Instant::now();
        let frame_time = (now - self.last_frame)
            .as_secs_f32()
            .min(config::MAX_FRAME_TIME);
        self.last_frame = now;
        self.input.poll_gamepad();
        if !self.freeze_splash {
            self.game.tick_splash(frame_time);
        }

        if self.input.take_pause() {
            self.game.toggle_pause();
        }

        if self.game.screen == GameScreen::Playing && !self.freeze_simulation {
            self.accumulator += frame_time;
            while self.accumulator >= config::FIXED_DT {
                let mut movement = if self.autoplay {
                    glam::Vec2::Y
                } else {
                    self.input.movement()
                };
                if self.save.settings.invert_x {
                    movement.x = -movement.x;
                }
                if self.save.settings.invert_y {
                    movement.y = -movement.y;
                }
                self.game.fixed_step(
                    movement,
                    self.input.take_jump(),
                    self.save.settings.camera_sensitivity,
                    self.save.settings.ball_feel,
                    config::FIXED_DT,
                );
                self.accumulator -= config::FIXED_DT;
            }
        } else {
            self.accumulator = self.accumulator.min(config::FIXED_DT);
        }

        self.game
            .update_streaming(self.save.settings.graphics_quality);
        let (incoming, outgoing) = self.game.take_terrain_changes();
        self.renderer.sync_terrain(incoming, outgoing);
        self.audio.update(
            frame_time,
            self.game.ball.speed(),
            self.game.ball.grounded && self.game.screen == GameScreen::Playing,
            self.game.screen,
        );
        for event in self.game.take_audio_events() {
            self.audio.event(event);
        }
        let progress = self.game.take_progress_delta();
        self.save.progression.total_pickups += progress.pickups;
        self.save.progression.total_distance += progress.distance;
        self.save.progression.completed_runs += progress.completed_runs;
        self.save.progression.party_pickups += progress.party_pickups;
        self.save.progression.best_streak =
            self.save.progression.best_streak.max(progress.best_streak);
        self.progression_save_timer += frame_time;
        if self.progression_save_timer >= 10.0
            || progress.party_pickups > 0
            || progress.completed_runs > 0
        {
            self.save.store();
            self.progression_save_timer = 0.0;
        }
        self.update_records();

        let raw_input = self.egui_state.take_egui_input(window);
        let mut action = UiAction::None;
        let output = self.egui_context.run(raw_input, |ctx| {
            action = ui::draw(
                ctx,
                &self.game,
                &mut self.save,
                &self.audio,
                &mut self.ui_assets,
            );
        });
        self.egui_state
            .handle_platform_output(window, output.platform_output);
        let paint_jobs = self
            .egui_context
            .tessellate(output.shapes, output.pixels_per_point);
        let should_quit = self.apply_action(action, window);
        let interpolation = (self.accumulator / config::FIXED_DT).clamp(0.0, 1.0);
        match self.renderer.render(
            &self.game,
            interpolation,
            self.save.settings.camera_zoom,
            &paint_jobs,
            &output.textures_delta,
            output.pixels_per_point,
        ) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.renderer.resize(window.inner_size())
            }
            Err(wgpu::SurfaceError::OutOfMemory) => return true,
            Err(wgpu::SurfaceError::Timeout) => log::warn!("A frame timed out"),
        }
        should_quit
    }

    fn update_records(&mut self) {
        let previous_score = self.save.best_score;
        let previous_distance = self.save.longest_distance;
        self.save.best_score = self.save.best_score.max(self.game.score);
        self.save.longest_distance = self.save.longest_distance.max(self.game.distance);
        if self.game.screen == GameScreen::GameOver
            && (self.save.best_score != previous_score
                || self.save.longest_distance != previous_distance)
        {
            self.save.store();
        }
    }

    fn apply_action(&mut self, action: UiAction, window: &Window) -> bool {
        match action {
            UiAction::None => {}
            UiAction::PlayNew => {
                self.game
                    .start_new_seed(self.save.settings.terrain_intensity);
                if !self.save.tutorial_seen {
                    self.game.screen = GameScreen::Tutorial;
                }
            }
            UiAction::RestartNew => self
                .game
                .start_new_seed(self.save.settings.terrain_intensity),
            UiAction::RestartSame => self
                .game
                .start_same_seed(self.save.settings.terrain_intensity),
            UiAction::Resume => self.game.screen = GameScreen::Playing,
            UiAction::OpenTutorial => {
                self.game
                    .start_new_seed(self.save.settings.terrain_intensity);
                self.game.screen = GameScreen::Tutorial;
            }
            UiAction::BeginRun => {
                self.save.tutorial_seen = true;
                self.save.store();
                self.game.screen = GameScreen::Playing;
            }
            UiAction::OpenSettings => {
                self.game.screen = if self.game.screen == GameScreen::Paused {
                    GameScreen::PauseSettings
                } else {
                    GameScreen::Settings
                };
            }
            UiAction::OpenAchievements => self.game.screen = GameScreen::Achievements,
            UiAction::BackToMenu => {
                self.game.screen = GameScreen::Menu;
                self.save.store();
            }
            UiAction::ApplySettings => match self.game.screen {
                GameScreen::PauseSettings
                    if self.save.settings.visual_style != self.game.visual_style =>
                {
                    self.game.screen = GameScreen::ThemeRestartWarning;
                }
                GameScreen::PauseSettings => {
                    apply_video_settings(window, &self.save);
                    self.audio.apply_settings(&self.save.settings);
                    if (self.save.settings.terrain_intensity - self.game.terrain_intensity).abs()
                        > f32::EPSILON
                    {
                        self.game
                            .apply_live_terrain_intensity(self.save.settings.terrain_intensity);
                    }
                    self.game.apply_trail_settings(
                        self.save.settings.trail_style,
                        self.save.settings.trail_deformation,
                    );
                    self.save.store();
                    self.game.screen = GameScreen::Paused;
                }
                GameScreen::Settings => {
                    apply_video_settings(window, &self.save);
                    self.audio.apply_settings(&self.save.settings);
                    self.game.apply_terrain_settings(
                        self.save.settings.terrain_intensity,
                        self.save.settings.visual_style,
                    );
                    self.game.apply_trail_settings(
                        self.save.settings.trail_style,
                        self.save.settings.trail_deformation,
                    );
                    self.save.store();
                    self.game.screen = GameScreen::Menu;
                }
                _ => {}
            },
            UiAction::ConfirmThemeRestart => {
                apply_video_settings(window, &self.save);
                self.audio.apply_settings(&self.save.settings);
                self.game.restart_same_seed_with_settings(
                    self.save.settings.terrain_intensity,
                    self.save.settings.visual_style,
                );
                self.game.apply_trail_settings(
                    self.save.settings.trail_style,
                    self.save.settings.trail_deformation,
                );
                self.save.store();
            }
            UiAction::CancelThemeRestart => self.game.screen = GameScreen::PauseSettings,
            UiAction::Quit => return true,
        }
        false
    }
}

fn apply_video_settings(window: &Window, save: &SaveData) {
    match save.settings.screen_mode {
        ScreenMode::Windowed => {
            window.set_fullscreen(None);
            let requested_size =
                PhysicalSize::new(save.settings.resolution[0], save.settings.resolution[1]);
            let _ = window.request_inner_size(requested_size);
            center_window(window, requested_size);
        }
        ScreenMode::Borderless => window.set_fullscreen(Some(Fullscreen::Borderless(None))),
    }
}

#[cfg(target_os = "macos")]
fn center_window(window: &Window, _requested_size: PhysicalSize<u32>) {
    use objc2::{msg_send, runtime::AnyObject};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let ns_view = handle.ns_view.as_ptr().cast::<AnyObject>();
    // SAFETY: Winit owns this live NSView for the lifetime of `window`; these standard AppKit
    // messages retrieve its NSWindow and centre that same window on its current display.
    unsafe {
        let ns_window: *mut AnyObject = msg_send![ns_view, window];
        if !ns_window.is_null() {
            let _: () = msg_send![ns_window, center];
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn center_window(window: &Window, requested_size: PhysicalSize<u32>) {
    let Some(monitor) = window.current_monitor() else {
        return;
    };
    let monitor_size = monitor.size();
    let monitor_position = monitor.position();
    let x = i64::from(monitor_position.x)
        + ((i64::from(monitor_size.width) - i64::from(requested_size.width)) / 2).max(0);
    let y = i64::from(monitor_position.y)
        + ((i64::from(monitor_size.height) - i64::from(requested_size.height)) / 2).max(0);
    window.set_outer_position(winit::dpi::PhysicalPosition::new(x as i32, y as i32));
}
