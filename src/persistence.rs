use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenMode {
    #[default]
    Windowed,
    Borderless,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphicsQuality {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BallFeel {
    Precision,
    #[default]
    Responsive,
    Momentum,
}

impl BallFeel {
    pub const ALL: [Self; 3] = [Self::Precision, Self::Responsive, Self::Momentum];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Precision => "Precision",
            Self::Responsive => "Responsive",
            Self::Momentum => "Momentum",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Precision => "Measured pace, stronger grip and extra steering authority",
            Self::Responsive => "Quick torque and direct control — the recommended default",
            Self::Momentum => "Higher top speed, gentle steering and long rolling momentum",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VisualStyle {
    #[default]
    Classic,
    Vaporwave,
    Dark,
}

impl VisualStyle {
    pub const ALL: [Self; 3] = [Self::Classic, Self::Vaporwave, Self::Dark];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Vaporwave => "Vaporwave",
            Self::Dark => "Dark",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Classic => "The original clean white contour landscape",
            Self::Vaporwave => "Broad neon hills, deep violet valleys and a pearl player",
            Self::Dark => "Extreme charcoal banks, sweeping curves and a dramatic low camera",
        }
    }
}

/// Stable cosmetic identifiers are deliberately serialized as an enum so additional paid or
/// unlockable trail packs can be added later without changing the gameplay save format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrailStyle {
    Off,
    #[default]
    Smoke,
    Graphite,
    Neon,
    Prism,
}

impl TrailStyle {
    pub const ALL: [Self; 5] = [
        Self::Off,
        Self::Smoke,
        Self::Graphite,
        Self::Neon,
        Self::Prism,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Smoke => "Smoke",
            Self::Graphite => "Graphite",
            Self::Neon => "Neon",
            Self::Prism => "Prism",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub master_volume: f32,
    pub music_volume: f32,
    pub effects_volume: f32,
    pub shuffle_music: bool,
    pub camera_sensitivity: f32,
    pub camera_zoom: f32,
    pub terrain_intensity: f32,
    pub ball_feel: BallFeel,
    pub visual_style: VisualStyle,
    pub trail_style: TrailStyle,
    pub trail_deformation: bool,
    pub invert_x: bool,
    pub invert_y: bool,
    pub screen_mode: ScreenMode,
    pub resolution: [u32; 2],
    pub graphics_quality: GraphicsQuality,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            master_volume: 0.40,
            music_volume: 0.30,
            effects_volume: 0.75,
            shuffle_music: false,
            camera_sensitivity: 1.0,
            camera_zoom: 1.24,
            terrain_intensity: crate::config::TERRAIN_INTENSITY_DEFAULT,
            ball_feel: BallFeel::Responsive,
            visual_style: VisualStyle::Classic,
            trail_style: TrailStyle::Smoke,
            trail_deformation: true,
            invert_x: true,
            invert_y: false,
            screen_mode: ScreenMode::Windowed,
            resolution: [2560, 1080],
            graphics_quality: GraphicsQuality::Medium,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Progression {
    pub total_pickups: u64,
    pub total_distance: f64,
    pub completed_runs: u64,
    pub party_pickups: u64,
    pub best_streak: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AchievementMetric {
    Pickups,
    Distance,
    Runs,
    PartyPickups,
    BestStreak,
}

#[derive(Clone, Copy, Debug)]
pub struct Achievement {
    pub tier: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub metric: AchievementMetric,
    pub target: f64,
}

pub const ACHIEVEMENTS: [Achievement; 10] = [
    Achievement {
        tier: "SHORT",
        title: "FIRST SPARK",
        description: "Collect your first sphere",
        metric: AchievementMetric::Pickups,
        target: 1.0,
    },
    Achievement {
        tier: "SHORT",
        title: "FIND YOUR FEET",
        description: "Travel 500 metres in total",
        metric: AchievementMetric::Distance,
        target: 500.0,
    },
    Achievement {
        tier: "SHORT",
        title: "CHAIN REACTION",
        description: "Reach a 5-pickup streak",
        metric: AchievementMetric::BestStreak,
        target: 5.0,
    },
    Achievement {
        tier: "MEDIUM",
        title: "MAGNETIC",
        description: "Collect 100 spheres",
        metric: AchievementMetric::Pickups,
        target: 100.0,
    },
    Achievement {
        tier: "MEDIUM",
        title: "TEN KILOMETRES",
        description: "Travel 10 km in total",
        metric: AchievementMetric::Distance,
        target: 10_000.0,
    },
    Achievement {
        tier: "MEDIUM",
        title: "PARTY STARTER",
        description: "Collect a PARTY sphere",
        metric: AchievementMetric::PartyPickups,
        target: 1.0,
    },
    Achievement {
        tier: "LONG",
        title: "A THOUSAND LIGHTS",
        description: "Collect 1,000 spheres",
        metric: AchievementMetric::Pickups,
        target: 1_000.0,
    },
    Achievement {
        tier: "LONG",
        title: "ENDLESS ROAD",
        description: "Travel 100 km in total",
        metric: AchievementMetric::Distance,
        target: 100_000.0,
    },
    Achievement {
        tier: "LONG",
        title: "VETERAN",
        description: "Finish 50 runs by falling or getting stuck",
        metric: AchievementMetric::Runs,
        target: 50.0,
    },
    Achievement {
        tier: "LONG",
        title: "AFTERPARTY",
        description: "Collect 25 PARTY spheres",
        metric: AchievementMetric::PartyPickups,
        target: 25.0,
    },
];

impl Progression {
    pub fn value(&self, metric: AchievementMetric) -> f64 {
        match metric {
            AchievementMetric::Pickups => self.total_pickups as f64,
            AchievementMetric::Distance => self.total_distance,
            AchievementMetric::Runs => self.completed_runs as f64,
            AchievementMetric::PartyPickups => self.party_pickups as f64,
            AchievementMetric::BestStreak => self.best_streak as f64,
        }
    }

    pub fn completed_achievements(&self) -> usize {
        ACHIEVEMENTS
            .iter()
            .filter(|achievement| self.value(achievement.metric) >= achievement.target)
            .count()
    }
}

const CURRENT_SAVE_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SaveData {
    #[serde(default = "legacy_save_version")]
    pub save_version: u32,
    pub settings: Settings,
    pub best_score: u64,
    pub longest_distance: f32,
    pub progression: Progression,
    pub tutorial_seen: bool,
}

impl Default for SaveData {
    fn default() -> Self {
        Self {
            save_version: CURRENT_SAVE_VERSION,
            settings: Settings::default(),
            best_score: 0,
            longest_distance: 0.0,
            progression: Progression::default(),
            tutorial_seen: false,
        }
    }
}

const fn legacy_save_version() -> u32 {
    0
}

impl SaveData {
    pub fn load() -> Self {
        let Some(path) = save_path() else {
            return Self::default();
        };
        fs::read_to_string(path)
            .ok()
            .and_then(|json| Self::decode(&json))
            .unwrap_or_default()
    }

    pub fn store(&self) {
        let Some(path) = save_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let temporary = path.with_extension("tmp");
            if fs::write(&temporary, json).is_ok() {
                let _ = fs::rename(temporary, path);
            }
        }
    }

    fn decode(json: &str) -> Option<Self> {
        let mut save: Self = serde_json::from_str(json).ok()?;
        if save.save_version < 1 {
            // Version zero shipped with 1280 × 800 as its untouched factory resolution. Migrate
            // only that exact legacy default; every other saved display choice remains respected.
            if save.settings.resolution == [1280, 800] {
                save.settings.resolution = [2560, 1080];
            }
            save.save_version = 1;
        }
        Some(save)
    }
}

fn save_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library/Application Support/Ridgeline/save.json"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|root| PathBuf::from(root).join("Ridgeline/save.json"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .map(|root| root.join("ridgeline/save.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_data_round_trip_preserves_records_and_settings() {
        let save = SaveData {
            save_version: CURRENT_SAVE_VERSION,
            settings: Settings {
                master_volume: 0.31,
                shuffle_music: true,
                camera_zoom: 1.38,
                terrain_intensity: 1.72,
                ball_feel: BallFeel::Momentum,
                visual_style: VisualStyle::Dark,
                trail_style: TrailStyle::Prism,
                trail_deformation: false,
                graphics_quality: GraphicsQuality::High,
                ..Settings::default()
            },
            best_score: 42_900,
            longest_distance: 1337.25,
            progression: Progression {
                total_pickups: 84,
                total_distance: 19_250.0,
                completed_runs: 7,
                party_pickups: 2,
                best_streak: 11,
            },
            tutorial_seen: true,
        };
        let json = serde_json::to_string(&save).unwrap();
        let decoded = SaveData::decode(&json).unwrap();
        assert_eq!(decoded.best_score, 42_900);
        assert!((decoded.longest_distance - 1337.25).abs() < f32::EPSILON);
        assert_eq!(decoded.settings.graphics_quality, GraphicsQuality::High);
        assert!((decoded.settings.master_volume - 0.31).abs() < f32::EPSILON);
        assert!(decoded.settings.shuffle_music);
        assert!((decoded.settings.camera_zoom - 1.38).abs() < f32::EPSILON);
        assert!((decoded.settings.terrain_intensity - 1.72).abs() < f32::EPSILON);
        assert_eq!(decoded.settings.ball_feel, BallFeel::Momentum);
        assert_eq!(decoded.settings.visual_style, VisualStyle::Dark);
        assert_eq!(decoded.settings.trail_style, TrailStyle::Prism);
        assert!(!decoded.settings.trail_deformation);
        assert_eq!(decoded.progression.total_pickups, 84);
        assert_eq!(decoded.progression.party_pickups, 2);
        assert_eq!(decoded.progression.best_streak, 11);
        assert!(decoded.tutorial_seen);
    }

    #[test]
    fn factory_defaults_match_the_packaged_experience() {
        let settings = Settings::default();
        assert!((settings.master_volume - 0.40).abs() < f32::EPSILON);
        assert!((settings.music_volume - 0.30).abs() < f32::EPSILON);
        assert!((settings.effects_volume - 0.75).abs() < f32::EPSILON);
        assert!(!settings.shuffle_music);
        assert!((settings.camera_zoom - 1.24).abs() < f32::EPSILON);
        assert_eq!(
            settings.terrain_intensity,
            crate::config::TERRAIN_INTENSITY_DEFAULT
        );
        assert_eq!(settings.trail_style, TrailStyle::Smoke);
        assert_eq!(settings.ball_feel, BallFeel::Responsive);
        assert_eq!(settings.visual_style, VisualStyle::Classic);
        assert!(settings.trail_deformation);
        assert!(settings.invert_x);
        assert!(!settings.invert_y);
        assert_eq!(settings.resolution, [2560, 1080]);
    }

    #[test]
    fn legacy_default_resolution_migrates_once_to_ultrawide() {
        let legacy = r#"{
            "settings": { "resolution": [1280, 800] },
            "best_score": 900
        }"#;
        let migrated = SaveData::decode(legacy).unwrap();
        assert_eq!(migrated.save_version, CURRENT_SAVE_VERSION);
        assert_eq!(migrated.settings.resolution, [2560, 1080]);
        assert_eq!(migrated.best_score, 900);

        let deliberately_standard = r#"{
            "save_version": 1,
            "settings": { "resolution": [1280, 800] }
        }"#;
        assert_eq!(
            SaveData::decode(deliberately_standard)
                .unwrap()
                .settings
                .resolution,
            [1280, 800]
        );
    }

    #[test]
    fn corrupt_save_falls_back_safely() {
        assert!(SaveData::decode("{not valid json").is_none());
    }

    #[test]
    fn achievement_completion_is_derived_from_saved_progress() {
        let progression = Progression {
            total_pickups: 100,
            total_distance: 10_000.0,
            completed_runs: 1,
            party_pickups: 1,
            best_streak: 5,
        };
        assert_eq!(progression.completed_achievements(), 6);
    }
}
