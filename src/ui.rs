use crate::{
    audio::{AudioSystem, NowPlaying},
    game::{Game, GameScreen},
    persistence::{
        AchievementMetric, BallFeel, GraphicsQuality, SaveData, ScreenMode, TrailStyle,
        VisualStyle, ACHIEVEMENTS,
    },
};
use egui::{
    Align, Align2, Color32, FontId, Layout, Margin, RichText, Rounding, Stroke, TextureHandle, Vec2,
};

#[derive(Default)]
pub struct UiAssets {
    music_track: Option<usize>,
    album_art: Option<TextureHandle>,
}

impl UiAssets {
    fn sync_music(&mut self, ctx: &egui::Context, audio: &AudioSystem) {
        let next_track = audio.now_playing().map(|track| track.id);
        if next_track == self.music_track {
            return;
        }
        self.music_track = next_track;
        self.album_art = next_track.and_then(|track_id| {
            let bytes = audio.artwork_bytes(track_id)?;
            let image = image::load_from_memory(&bytes).ok()?.resize_to_fill(
                128,
                128,
                image::imageops::FilterType::Triangle,
            );
            let image = image.to_rgba8();
            let size = [image.width() as usize, image.height() as usize];
            let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
            Some(ctx.load_texture(
                format!("album-art-{track_id}"),
                color_image,
                egui::TextureOptions::LINEAR,
            ))
        });
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiAction {
    #[default]
    None,
    PlayNew,
    RestartSame,
    RestartNew,
    Resume,
    OpenTutorial,
    BeginRun,
    OpenSettings,
    OpenAchievements,
    BackToMenu,
    ApplySettings,
    ConfirmThemeRestart,
    CancelThemeRestart,
    Quit,
}

pub fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.panel_fill = Color32::TRANSPARENT;
    style.visuals.window_fill = Color32::from_rgba_unmultiplied(248, 248, 248, 244);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(242, 242, 242);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_gray(55));
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(224, 24, 31);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(190, 12, 20);
    style.spacing.button_padding = Vec2::new(18.0, 9.0);
    style.spacing.item_spacing = Vec2::new(9.0, 10.0);
    ctx.set_style(style);
}

pub fn draw(
    ctx: &egui::Context,
    game: &Game,
    save: &mut SaveData,
    audio: &AudioSystem,
    assets: &mut UiAssets,
) -> UiAction {
    assets.sync_music(ctx, audio);
    let action = match game.screen {
        GameScreen::Splash => splash(ctx, game),
        GameScreen::Menu => menu(ctx, save, audio.now_playing(), assets.album_art.as_ref()),
        GameScreen::Tutorial => tutorial(ctx),
        GameScreen::Settings => settings(ctx, save, false),
        GameScreen::PauseSettings => settings(ctx, save, true),
        GameScreen::ThemeRestartWarning => theme_restart_warning(ctx, game, save),
        GameScreen::Achievements => achievements(ctx, save),
        GameScreen::Playing => hud(ctx, game),
        GameScreen::Paused => pause(ctx, game),
        GameScreen::GameOver => game_over(ctx, game, save),
    };
    if matches!(
        game.screen,
        GameScreen::Playing
            | GameScreen::Paused
            | GameScreen::PauseSettings
            | GameScreen::ThemeRestartWarning
            | GameScreen::GameOver
    ) {
        now_playing(ctx, audio.now_playing(), assets.album_art.as_ref());
    }
    action
}

fn splash(ctx: &egui::Context, game: &Game) -> UiAction {
    egui::CentralPanel::default()
        .frame(egui::Frame::none())
        .show(ctx, |ui| {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space(ui.available_height() * 0.39);
                ui.label(
                    RichText::new("RIDGELINE")
                        .font(FontId::proportional(38.0))
                        .strong()
                        .color(Color32::from_gray(24)),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("MOMENTUM, DRAWN IN LINES")
                        .size(11.0)
                        .color(Color32::from_gray(105))
                        .extra_letter_spacing(3.2),
                );
                let opacity = ((game.elapsed * 2.2).sin() * 0.25 + 0.6).clamp(0.0, 1.0);
                ui.add_space(42.0);
                ui.spinner();
                ui.label(
                    RichText::new("PREPARING THE TERRAIN")
                        .size(9.0)
                        .color(Color32::from_white_alpha((opacity * 140.0) as u8)),
                );
            });
        });
    UiAction::None
}

fn menu(
    ctx: &egui::Context,
    save: &SaveData,
    track: Option<NowPlaying<'_>>,
    album_art: Option<&TextureHandle>,
) -> UiAction {
    let mut action = UiAction::None;
    centered_card(ctx, 350.0, |ui| {
        ui.label(
            RichText::new("RIDGELINE")
                .font(FontId::proportional(36.0))
                .strong()
                .color(Color32::from_gray(22)),
        );
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("AN ENDLESS STUDY IN MOMENTUM")
                    .size(9.0)
                    .color(Color32::from_gray(112))
                    .extra_letter_spacing(1.6),
            );
            ui.separator();
            ui.hyperlink_to(
                RichText::new("☕ SUPPORT")
                    .size(8.0)
                    .strong()
                    .color(Color32::from_rgb(196, 24, 32)),
                "https://buymeacoffee.com/CHCOfficial",
            );
        });
        ui.label(
            RichText::new("INSPIRED BY @CHRISLAKIN")
                .size(7.5)
                .strong()
                .color(Color32::from_gray(132))
                .extra_letter_spacing(1.1),
        );
        ui.add_space(16.0);
        if primary_button(ui, "PLAY").clicked() {
            action = UiAction::PlayNew;
        }
        if secondary_button(ui, "HOW TO PLAY").clicked() {
            action = UiAction::OpenTutorial;
        }
        if secondary_button(ui, "SETTINGS").clicked() {
            action = UiAction::OpenSettings;
        }
        if secondary_button(
            ui,
            &format!(
                "ACHIEVEMENTS   {}/{}",
                save.progression.completed_achievements(),
                ACHIEVEMENTS.len()
            ),
        )
        .clicked()
        {
            action = UiAction::OpenAchievements;
        }
        if secondary_button(ui, "QUIT").clicked() {
            action = UiAction::Quit;
        }
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            stat(ui, "BEST", format_number(save.best_score));
            ui.separator();
            stat(ui, "LONGEST", format!("{:.0} m", save.longest_distance));
            if let Some(track) = track {
                ui.separator();
                if let Some(texture) = album_art {
                    ui.image((texture.id(), Vec2::splat(30.0)));
                }
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("MENU THEME")
                            .size(6.5)
                            .strong()
                            .extra_letter_spacing(1.0)
                            .color(Color32::from_rgb(196, 24, 32)),
                    );
                    ui.label(RichText::new(track.title).size(8.0).strong());
                    ui.label(
                        RichText::new(format!(
                            "{}  •  {:02}/{:02}",
                            track.artist, track.number, track.total
                        ))
                        .size(6.5)
                        .color(Color32::from_gray(118)),
                    )
                    .on_hover_text(track.album);
                });
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new("WASD / ARROWS  •  LEFT STICK  •  SPACE TO JUMP")
                .size(9.0)
                .color(Color32::from_gray(125)),
        );
    });
    action
}

fn tutorial(ctx: &egui::Context) -> UiAction {
    let mut action = UiAction::None;
    dim_background(ctx);
    centered_card(ctx, 520.0, |ui| {
        ui.spacing_mut().item_spacing.y = 7.0;
        ui.label(
            RichText::new("HOW TO PLAY")
                .font(FontId::proportional(28.0))
                .strong(),
        );
        ui.label(
            RichText::new("ROLL  •  COLLECT  •  SURVIVE")
                .size(9.5)
                .color(Color32::from_gray(112))
                .extra_letter_spacing(2.0),
        );
        ui.add_space(7.0);
        ui.horizontal(|ui| {
            keycap(ui, "WASD / ARROWS");
            ui.label(
                RichText::new("STEER")
                    .size(9.0)
                    .color(Color32::from_gray(100)),
            );
            keycap(ui, "SPACE");
            ui.label(
                RichText::new("JUMP")
                    .size(9.0)
                    .color(Color32::from_gray(100)),
            );
            keycap(ui, "ESC");
            ui.label(
                RichText::new("PAUSE")
                    .size(9.0)
                    .color(Color32::from_gray(100)),
            );
        });
        ui.add_space(4.0);
        tutorial_rule(
            ui,
            "01",
            "FOLLOW THE LAND",
            "Build momentum through the valleys and steer across the slopes. Gravity is part of the route.",
        );
        tutorial_rule(
            ui,
            "02",
            "COLLECT THE LIGHTS",
            "Gold spheres score points and build streaks. A rainbow PARTY sphere gives 4× points for 30 seconds.",
        );
        tutorial_rule(
            ui,
            "03",
            "FINISHING A RUN",
            "Avoid the dark tears in the terrain. Falling through one ends the run and records it toward achievements.",
        );
        ui.add_space(7.0);
        if primary_button(ui, "START RUN").clicked() {
            action = UiAction::BeginRun;
        }
    });
    action
}

fn keycap(ui: &mut egui::Ui, text: &str) {
    egui::Frame::none()
        .fill(Color32::from_rgb(235, 235, 235))
        .stroke(Stroke::new(1.0, Color32::from_gray(205)))
        .rounding(Rounding::same(3.0))
        .inner_margin(Margin::symmetric(6.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).monospace().strong().size(9.0));
        });
}

fn tutorial_rule(ui: &mut egui::Ui, number: &str, title: &str, description: &str) {
    egui::Frame::none()
        .fill(Color32::from_rgb(244, 244, 244))
        .rounding(Rounding::same(4.0))
        .inner_margin(Margin::symmetric(11.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(number)
                        .monospace()
                        .size(10.0)
                        .strong()
                        .color(Color32::from_rgb(214, 20, 28)),
                );
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(10.0).strong());
                    ui.label(
                        RichText::new(description)
                            .size(9.2)
                            .color(Color32::from_gray(92)),
                    );
                });
            });
        });
}

fn settings(ctx: &egui::Context, save: &mut SaveData, paused_run: bool) -> UiAction {
    let mut action = UiAction::None;
    centered_card(ctx, 580.0, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(
            RichText::new("SETTINGS")
                .font(FontId::proportional(26.0))
                .strong(),
        );
        if paused_run {
            ui.label(
                RichText::new("PAUSED RUN  •  CHANGES APPLY WITHOUT LOSING PROGRESS")
                    .size(7.5)
                    .strong()
                    .color(Color32::from_gray(115))
                    .extra_letter_spacing(0.8),
            );
        }
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            let (left, right) = columns.split_at_mut(1);
            let left = &mut left[0];
            let right = &mut right[0];

            settings_section(left, "AUDIO & PLAYBACK");
            value_slider(left, "MASTER", &mut save.settings.master_volume);
            value_slider(left, "MUSIC", &mut save.settings.music_volume);
            value_slider(left, "EFFECTS", &mut save.settings.effects_volume);
            left.horizontal(|ui| {
                ui.checkbox(&mut save.settings.shuffle_music, "SHUFFLE MUSIC");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new("38 TRACKS")
                            .monospace()
                            .size(9.0)
                            .color(Color32::from_gray(112)),
                    );
                });
            });
            left.add_space(8.0);
            settings_section(left, "PLAY & CAMERA");
            left.horizontal(|ui| {
                ui.label(RichText::new("BALL FEEL").size(11.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_source("ball-feel")
                        .selected_text(save.settings.ball_feel.label())
                        .show_ui(ui, |ui| {
                            for feel in BallFeel::ALL {
                                ui.selectable_value(
                                    &mut save.settings.ball_feel,
                                    feel,
                                    feel.label(),
                                )
                                .on_hover_text(feel.description());
                            }
                        })
                        .response
                        .on_hover_text(save.settings.ball_feel.description());
                });
            });
            left.horizontal(|ui| {
                ui.label(RichText::new("VISUAL STYLE").size(11.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_source("visual-style")
                        .selected_text(save.settings.visual_style.label())
                        .show_ui(ui, |ui| {
                            for style in VisualStyle::ALL {
                                ui.selectable_value(
                                    &mut save.settings.visual_style,
                                    style,
                                    style.label(),
                                )
                                .on_hover_text(style.description());
                            }
                        })
                        .response
                        .on_hover_text(save.settings.visual_style.description());
                });
            });
            left.horizontal(|ui| {
                ui.label(RichText::new("CAMERA FOLLOW").size(11.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(
                        egui::Slider::new(&mut save.settings.camera_sensitivity, 0.55..=1.65)
                            .show_value(false),
                    );
                });
            });
            left.horizontal(|ui| {
                ui.label(RichText::new("CAMERA ZOOM").size(11.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(
                        egui::Slider::new(&mut save.settings.camera_zoom, 0.72..=1.55)
                            .show_value(false),
                    )
                    .on_hover_text("Lower values zoom in; higher values reveal more terrain");
                    ui.label(
                        RichText::new(format!(
                            "{:>3}%",
                            (save.settings.camera_zoom * 100.0) as u32
                        ))
                        .monospace()
                        .size(10.0),
                    );
                });
            });

            settings_section(right, "WORLD & DISPLAY");
            right.horizontal(|ui| {
                ui.label(RichText::new("PEAKS & VALLEYS").size(11.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(
                        egui::Slider::new(&mut save.settings.terrain_intensity, 0.60..=2.60)
                            .show_value(false),
                    )
                    .on_hover_text("Adjusts the height and depth of generated terrain formations");
                    ui.label(
                        RichText::new(format!(
                            "{:>3}%",
                            (save.settings.terrain_intensity * 100.0) as u32
                        ))
                        .monospace()
                        .size(10.0),
                    );
                });
            });
            right.horizontal(|ui| {
                ui.label(RichText::new("SURFACE TRAIL").size(11.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_source("surface-trail")
                        .selected_text(save.settings.trail_style.label())
                        .show_ui(ui, |ui| {
                            for style in TrailStyle::ALL {
                                ui.selectable_value(
                                    &mut save.settings.trail_style,
                                    style,
                                    style.label(),
                                );
                            }
                        });
                });
            });
            right.horizontal(|ui| {
                ui.checkbox(
                    &mut save.settings.trail_deformation,
                    "TESSELLATED SURFACE IMPRINT",
                )
                .on_hover_text(
                    "Visually depresses the rendered terrain beneath nearby trail marks",
                );
            });
            right.horizontal(|ui| {
                ui.checkbox(&mut save.settings.invert_x, "INVERT X AXIS");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.checkbox(&mut save.settings.invert_y, "INVERT Y AXIS");
                });
            });
            right.horizontal(|ui| {
                ui.label(RichText::new("SCREEN").size(11.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_source("screen-mode")
                        .selected_text(match save.settings.screen_mode {
                            ScreenMode::Windowed => "Windowed",
                            ScreenMode::Borderless => "Borderless",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut save.settings.screen_mode,
                                ScreenMode::Windowed,
                                "Windowed",
                            );
                            ui.selectable_value(
                                &mut save.settings.screen_mode,
                                ScreenMode::Borderless,
                                "Borderless",
                            );
                        });
                });
            });
            right.horizontal(|ui| {
                ui.label(RichText::new("RESOLUTION").size(11.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_source("resolution")
                        .selected_text(if save.settings.resolution == [2560, 1080] {
                            "2560 × 1080  •  21:9".to_owned()
                        } else {
                            format!(
                                "{} × {}",
                                save.settings.resolution[0], save.settings.resolution[1]
                            )
                        })
                        .show_ui(ui, |ui| {
                            for (size, label) in [
                                ([1280, 720], "1280 × 720"),
                                ([1280, 800], "1280 × 800"),
                                ([1440, 900], "1440 × 900"),
                                ([1920, 1080], "1920 × 1080"),
                                ([2560, 1080], "2560 × 1080  •  21:9"),
                            ] {
                                ui.selectable_value(&mut save.settings.resolution, size, label);
                            }
                        });
                });
            });
            right.horizontal(|ui| {
                ui.label(RichText::new("QUALITY").size(11.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_source("quality")
                        .selected_text(match save.settings.graphics_quality {
                            GraphicsQuality::Low => "Low",
                            GraphicsQuality::Medium => "Medium",
                            GraphicsQuality::High => "High",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut save.settings.graphics_quality,
                                GraphicsQuality::Low,
                                "Low",
                            );
                            ui.selectable_value(
                                &mut save.settings.graphics_quality,
                                GraphicsQuality::Medium,
                                "Medium",
                            );
                            ui.selectable_value(
                                &mut save.settings.graphics_quality,
                                GraphicsQuality::High,
                                "High",
                            );
                        });
                });
            });
        });
        ui.add_space(10.0);
        if primary_button(ui, "APPLY & RETURN").clicked() {
            action = UiAction::ApplySettings;
        }
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            ui.hyperlink_to(
                RichText::new("CODE").size(9.0).strong(),
                "https://github.com/CHCOfficial",
            );
            ui.label(RichText::new("•").size(8.0).color(Color32::from_gray(150)));
            ui.hyperlink_to(
                RichText::new("GRAPHICS").size(9.0).strong(),
                "https://www.deviantart.com/chcofficial",
            );
            ui.label(RichText::new("•").size(8.0).color(Color32::from_gray(150)));
            ui.hyperlink_to(
                RichText::new("AUDIO").size(9.0).strong(),
                "https://suno.com/@artfulexpchc",
            );
        });
    });
    action
}

fn theme_restart_warning(ctx: &egui::Context, game: &Game, save: &SaveData) -> UiAction {
    let mut action = UiAction::None;
    dim_background(ctx);
    centered_card(ctx, 390.0, |ui| {
        ui.label(
            RichText::new("RESTART THIS RUN?")
                .font(FontId::proportional(28.0))
                .strong(),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!(
                "Changing the theme from {} to {} alters the terrain and gameplay.",
                game.visual_style.label(),
                save.settings.visual_style.label()
            ))
            .size(10.0)
            .color(Color32::from_gray(72)),
        );
        ui.label(
            RichText::new(
                "Your current score and distance will be reset. The same seed will be used.",
            )
            .size(9.5)
            .color(Color32::from_gray(98)),
        );
        ui.add_space(16.0);
        if primary_button(ui, "APPLY THEME & RESTART").clicked() {
            action = UiAction::ConfirmThemeRestart;
        }
        if secondary_button(ui, "CANCEL").clicked() {
            action = UiAction::CancelThemeRestart;
        }
    });
    action
}

fn settings_section(ui: &mut egui::Ui, title: &str) {
    ui.label(
        RichText::new(title)
            .size(8.5)
            .strong()
            .extra_letter_spacing(1.4)
            .color(Color32::from_rgb(196, 24, 32)),
    );
    ui.add_space(2.0);
}

fn now_playing(
    ctx: &egui::Context,
    track: Option<NowPlaying<'_>>,
    album_art: Option<&TextureHandle>,
) {
    let Some(track) = track else { return };
    egui::Area::new(egui::Id::new("now-playing"))
        .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-18.0, -16.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(Color32::from_black_alpha(186))
                .stroke(Stroke::new(1.0, Color32::from_white_alpha(32)))
                .rounding(Rounding::same(5.0))
                .inner_margin(Margin::symmetric(8.0, 7.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(texture) = album_art {
                            ui.image((texture.id(), Vec2::splat(44.0)));
                        }
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(if track.menu_theme {
                                    "MENU THEME"
                                } else {
                                    "NOW PLAYING"
                                })
                                .size(7.5)
                                .strong()
                                .extra_letter_spacing(1.3)
                                .color(Color32::from_rgb(236, 82, 113)),
                            );
                            ui.label(
                                RichText::new(track.title)
                                    .size(10.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{}  •  {:02}/{:02}",
                                    track.artist, track.number, track.total
                                ))
                                .size(8.0)
                                .color(Color32::from_gray(190)),
                            )
                            .on_hover_text(track.album);
                        });
                    });
                });
        });
}

fn hud(ctx: &egui::Context, game: &Game) -> UiAction {
    let (label_color, value_color, seed_color) = match game.visual_style {
        VisualStyle::Classic => (
            Color32::from_gray(120),
            Color32::from_gray(25),
            Color32::from_gray(115),
        ),
        VisualStyle::Vaporwave => (
            Color32::from_rgb(72, 225, 246),
            Color32::from_rgb(255, 225, 250),
            Color32::from_rgb(207, 120, 226),
        ),
        VisualStyle::Dark => (
            Color32::from_gray(165),
            Color32::from_gray(238),
            Color32::from_gray(145),
        ),
    };
    egui::TopBottomPanel::top("hud")
        .show_separator_line(false)
        .frame(egui::Frame::none().inner_margin(Margin::symmetric(18.0, 8.0)))
        .show(ctx, |ui| {
            ui.columns(4, |columns| {
                hud_stat(
                    &mut columns[0],
                    "SCORE",
                    format_number(game.score),
                    label_color,
                    value_color,
                );
                hud_stat(
                    &mut columns[1],
                    "STREAK",
                    if game.streak > 1 {
                        format!("×{}", game.streak)
                    } else {
                        "—".into()
                    },
                    label_color,
                    value_color,
                );
                hud_stat(
                    &mut columns[2],
                    "DISTANCE",
                    format!("{:.0} m", game.distance),
                    label_color,
                    value_color,
                );
                hud_stat(
                    &mut columns[3],
                    "SPEED",
                    format!("{:02.0} m/s", game.ball.speed()),
                    label_color,
                    value_color,
                );
            });
        });
    if game.party_active() {
        egui::Area::new(egui::Id::new("party-status"))
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 55.0))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(38, 7, 70, 220))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(Margin::symmetric(12.0, 5.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!(
                                "PARTY  •  {:02.0}s  •  4× POINTS",
                                game.party_time.ceil()
                            ))
                            .size(10.0)
                            .strong()
                            .color(Color32::from_rgb(78, 244, 255)),
                        );
                    });
            });
    }
    egui::Area::new(egui::Id::new("seed"))
        .anchor(Align2::LEFT_BOTTOM, Vec2::new(22.0, -18.0))
        .show(ctx, |ui| {
            ui.label(
                RichText::new(format!("SEED  {:016X}", game.seed))
                    .monospace()
                    .size(9.0)
                    .color(seed_color),
            )
        });
    if game.recovery_notice > 0.0 {
        egui::Area::new(egui::Id::new("recovery"))
            .anchor(Align2::CENTER_CENTER, Vec2::new(0.0, -90.0))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("RECOVERY GRACE")
                        .size(12.0)
                        .strong()
                        .color(Color32::from_rgb(205, 20, 28)),
                )
            });
    }
    UiAction::None
}

fn achievements(ctx: &egui::Context, save: &SaveData) -> UiAction {
    let mut action = UiAction::None;
    centered_card(ctx, 500.0, |ui| {
        ui.spacing_mut().item_spacing.y = 5.0;
        ui.label(
            RichText::new("ACHIEVEMENTS")
                .font(FontId::proportional(24.0))
                .strong(),
        );
        ui.label(
            RichText::new(format!(
                "{} OF {} COMPLETE",
                save.progression.completed_achievements(),
                ACHIEVEMENTS.len()
            ))
            .size(9.0)
            .color(Color32::from_gray(110))
            .extra_letter_spacing(1.5),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            stat(ui, "PICKUPS", format_number(save.progression.total_pickups));
            stat(
                ui,
                "DISTANCE",
                format!("{:.1} km", save.progression.total_distance / 1000.0),
            );
            stat(ui, "PARTIES", format_number(save.progression.party_pickups));
            stat(
                ui,
                "BEST STREAK",
                format!("×{}", save.progression.best_streak),
            );
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .max_height(205.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for achievement in ACHIEVEMENTS {
                    let value = save.progression.value(achievement.metric);
                    let complete = value >= achievement.target;
                    egui::Frame::none()
                        .fill(if complete {
                            Color32::from_rgb(235, 247, 241)
                        } else {
                            Color32::from_rgb(244, 244, 244)
                        })
                        .rounding(Rounding::same(4.0))
                        .inner_margin(Margin::symmetric(10.0, 7.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{}  •  {}",
                                            achievement.tier, achievement.title
                                        ))
                                        .size(9.5)
                                        .strong()
                                        .color(
                                            if complete {
                                                Color32::from_rgb(20, 125, 82)
                                            } else {
                                                Color32::from_gray(45)
                                            },
                                        ),
                                    );
                                    ui.label(
                                        RichText::new(achievement.description)
                                            .size(9.0)
                                            .color(Color32::from_gray(105)),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(achievement_progress(
                                            value,
                                            achievement.target,
                                            achievement.metric,
                                        ))
                                        .monospace()
                                        .size(9.0),
                                    );
                                });
                            });
                            ui.add(
                                egui::ProgressBar::new(
                                    (value / achievement.target).clamp(0.0, 1.0) as f32,
                                )
                                .desired_width(ui.available_width())
                                .desired_height(3.0)
                                .fill(if complete {
                                    Color32::from_rgb(30, 175, 115)
                                } else {
                                    Color32::from_rgb(214, 20, 28)
                                }),
                            );
                        });
                    ui.add_space(4.0);
                }
            });
        ui.add_space(4.0);
        if primary_button(ui, "BACK").clicked() {
            action = UiAction::BackToMenu;
        }
    });
    action
}

fn achievement_progress(value: f64, target: f64, metric: AchievementMetric) -> String {
    match metric {
        AchievementMetric::Distance => format!("{:.1}/{:.1} km", value / 1000.0, target / 1000.0),
        AchievementMetric::BestStreak => format!("×{}/×{}", value as u64, target as u64),
        _ => format!("{}/{}", value as u64, target as u64),
    }
}

fn pause(ctx: &egui::Context, game: &Game) -> UiAction {
    let mut action = UiAction::None;
    dim_background(ctx);
    centered_card(ctx, 330.0, |ui| {
        ui.label(
            RichText::new("PAUSED")
                .font(FontId::proportional(32.0))
                .strong(),
        );
        ui.label(
            RichText::new(format!("{:016X}", game.seed))
                .monospace()
                .size(9.0)
                .color(Color32::from_gray(120)),
        );
        ui.add_space(18.0);
        if primary_button(ui, "CONTINUE").clicked() {
            action = UiAction::Resume;
        }
        if secondary_button(ui, "RESTART RUN").clicked() {
            action = UiAction::RestartSame;
        }
        if secondary_button(ui, "SETTINGS").clicked() {
            action = UiAction::OpenSettings;
        }
        if secondary_button(ui, "MAIN MENU").clicked() {
            action = UiAction::BackToMenu;
        }
    });
    action
}

fn game_over(ctx: &egui::Context, game: &Game, save: &SaveData) -> UiAction {
    let mut action = UiAction::None;
    dim_background(ctx);
    centered_card(ctx, 380.0, |ui| {
        ui.label(
            RichText::new("RUN COMPLETE")
                .font(FontId::proportional(31.0))
                .strong(),
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            stat(ui, "SCORE", format_number(game.score));
            ui.separator();
            stat(ui, "DISTANCE", format!("{:.0} m", game.distance));
            ui.separator();
            stat(ui, "BEST", format_number(save.best_score.max(game.score)));
        });
        ui.add_space(22.0);
        if primary_button(ui, "RETRY SAME SEED").clicked() {
            action = UiAction::RestartSame;
        }
        if secondary_button(ui, "NEW SEED").clicked() {
            action = UiAction::RestartNew;
        }
        if secondary_button(ui, "MAIN MENU").clicked() {
            action = UiAction::BackToMenu;
        }
    });
    action
}

fn centered_card(ctx: &egui::Context, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    egui::Area::new(egui::Id::new("center-card"))
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(250, 250, 250, 242))
                .stroke(Stroke::new(1.0, Color32::from_gray(218)))
                .rounding(Rounding::same(5.0))
                .inner_margin(Margin::same(22.0))
                .shadow(egui::epaint::Shadow {
                    offset: Vec2::new(0.0, 6.0),
                    blur: 24.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(25),
                })
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.with_layout(Layout::top_down(Align::Center), add);
                });
        });
}

fn dim_background(ctx: &egui::Context) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.rect_filled(ctx.screen_rect(), 0.0, Color32::from_black_alpha(54));
}

fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 34.0],
        egui::Button::new(
            RichText::new(text)
                .size(11.0)
                .strong()
                .color(Color32::WHITE),
        )
        .fill(Color32::from_rgb(214, 20, 28))
        .stroke(Stroke::NONE),
    )
}

fn secondary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 31.0],
        egui::Button::new(RichText::new(text).size(10.0).strong())
            .fill(Color32::from_gray(242))
            .stroke(Stroke::new(1.0, Color32::from_gray(210))),
    )
}

fn stat(ui: &mut egui::Ui, label: &str, value: String) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(label)
                .size(7.5)
                .strong()
                .color(Color32::from_gray(120))
                .extra_letter_spacing(1.5),
        );
        ui.label(
            RichText::new(value)
                .size(14.0)
                .strong()
                .color(Color32::from_gray(25)),
        );
    });
}

fn hud_stat(
    ui: &mut egui::Ui,
    label: &str,
    value: String,
    label_color: Color32,
    value_color: Color32,
) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(label)
                .size(7.5)
                .strong()
                .color(label_color)
                .extra_letter_spacing(1.5),
        );
        ui.label(RichText::new(value).size(14.0).strong().color(value_color));
    });
}

fn value_slider(ui: &mut egui::Ui, label: &str, value: &mut f32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(11.0).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Slider::new(value, 0.0..=1.0).show_value(false));
            ui.label(
                RichText::new(format!("{:>3}%", (*value * 100.0) as u32))
                    .monospace()
                    .size(10.0),
            );
        });
    });
}

fn format_number(value: u64) -> String {
    let raw = value.to_string();
    let mut output = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, ch) in raw.chars().enumerate() {
        if index > 0 && (raw.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(ch);
    }
    output
}
