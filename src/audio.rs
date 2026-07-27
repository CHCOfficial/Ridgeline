use crate::{
    game::{AudioEvent, GameScreen},
    persistence::Settings,
};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::{
    fs::{self, File},
    io::{BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MENU_THEME_NUMBER: u32 = 12;
const MUSIC_CROSSFADE_SECONDS: f32 = 3.2;
const MUSIC_GAIN: f32 = 0.82;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MusicMode {
    Menu,
    Game,
}

impl MusicMode {
    fn for_screen(screen: GameScreen) -> Self {
        match screen {
            GameScreen::Playing
            | GameScreen::Paused
            | GameScreen::PauseSettings
            | GameScreen::ThemeRestartWarning
            | GameScreen::GameOver => Self::Game,
            GameScreen::Splash
            | GameScreen::Menu
            | GameScreen::Tutorial
            | GameScreen::Settings
            | GameScreen::Achievements => Self::Menu,
        }
    }
}

#[derive(Clone, Debug)]
struct ArtworkLocation {
    offset: u64,
    length: usize,
}

#[derive(Clone, Debug)]
struct MusicTrack {
    number: u32,
    path: PathBuf,
    title: String,
    artist: String,
    album: String,
    artwork: Option<ArtworkLocation>,
}

#[derive(Clone, Copy, Debug)]
pub struct NowPlaying<'a> {
    pub id: usize,
    pub number: u32,
    pub total: usize,
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub menu_theme: bool,
}

struct MusicVoice {
    sink: Sink,
    track_index: usize,
    elapsed: f32,
    duration: Option<f32>,
    fade_elapsed: f32,
}

pub struct AudioSystem {
    _stream: Option<OutputStream>,
    stream_handle: Option<OutputStreamHandle>,
    active_music: Option<MusicVoice>,
    outgoing_music: Option<MusicVoice>,
    tracks: Vec<MusicTrack>,
    play_order: Vec<usize>,
    play_position: usize,
    music_mode: MusicMode,
    shuffle_music: bool,
    music_volume: f32,
    rolling: Option<Sink>,
    effects: Option<Sink>,
    intensity: Arc<AtomicU32>,
}

impl AudioSystem {
    pub fn new(settings: &Settings) -> Self {
        let tracks = discover_music_tracks();
        let play_order = (0..tracks.len()).collect();
        let intensity = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let Ok((stream, handle)) = OutputStream::try_default() else {
            return Self {
                _stream: None,
                stream_handle: None,
                active_music: None,
                outgoing_music: None,
                tracks,
                play_order,
                play_position: 0,
                music_mode: MusicMode::Menu,
                shuffle_music: settings.shuffle_music,
                music_volume: settings.master_volume * settings.music_volume * MUSIC_GAIN,
                rolling: None,
                effects: None,
                intensity,
            };
        };

        let rolling = Sink::try_new(&handle).ok();
        let effects = Sink::try_new(&handle).ok();
        if let Some(sink) = &rolling {
            sink.append(ProceduralRolling::new(intensity.clone()));
            sink.play();
        }

        let mut audio = Self {
            _stream: Some(stream),
            stream_handle: Some(handle),
            active_music: None,
            outgoing_music: None,
            tracks,
            play_order,
            play_position: 0,
            music_mode: MusicMode::Menu,
            shuffle_music: settings.shuffle_music,
            music_volume: settings.master_volume * settings.music_volume * MUSIC_GAIN,
            rolling,
            effects,
            intensity,
        };
        audio.apply_settings(settings);
        if let Some(menu_theme) = audio.menu_theme_index() {
            audio.transition_to(menu_theme);
        }
        audio
    }

    pub fn apply_settings(&mut self, settings: &Settings) {
        self.music_volume = settings.master_volume * settings.music_volume * MUSIC_GAIN;
        self.shuffle_music = settings.shuffle_music;
        self.refresh_music_volumes();
        if let Some(sink) = &self.rolling {
            sink.set_volume(settings.master_volume * settings.effects_volume * 0.30);
        }
        if let Some(sink) = &self.effects {
            sink.set_volume(settings.master_volume * settings.effects_volume * 0.55);
        }
    }

    pub fn update(&mut self, dt: f32, speed: f32, grounded: bool, screen: GameScreen) {
        let amount = if grounded {
            (speed / 35.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.intensity.store(amount.to_bits(), Ordering::Relaxed);

        let next_mode = MusicMode::for_screen(screen);
        if next_mode != self.music_mode {
            self.music_mode = next_mode;
            match next_mode {
                MusicMode::Menu => {
                    if let Some(menu_theme) = self.menu_theme_index() {
                        self.transition_to(menu_theme);
                    }
                }
                MusicMode::Game => self.begin_game_playlist(),
            }
        }

        self.update_music(dt);
    }

    pub fn event(&self, event: AudioEvent) {
        let Some(sink) = &self.effects else { return };
        let (frequency, duration, gain) = match event {
            AudioEvent::Collect { streak } => (690.0 + (streak.min(12) as f32 * 24.0), 0.16, 0.72),
            AudioEvent::Party => (520.0, 0.82, 0.78),
            AudioEvent::Jump => (230.0, 0.12, 0.42),
            AudioEvent::Recovery => (260.0, 0.34, 0.50),
            AudioEvent::GameOver => (118.0, 0.65, 0.48),
        };
        sink.append(Chime::new(frequency, duration, gain));
    }

    pub fn now_playing(&self) -> Option<NowPlaying<'_>> {
        let voice = self.active_music.as_ref()?;
        let track = self.tracks.get(voice.track_index)?;
        Some(NowPlaying {
            id: voice.track_index,
            number: track.number,
            total: self.tracks.len(),
            title: &track.title,
            artist: &track.artist,
            album: &track.album,
            menu_theme: self.music_mode == MusicMode::Menu,
        })
    }

    pub fn artwork_bytes(&self, track_id: usize) -> Option<Vec<u8>> {
        let track = self.tracks.get(track_id)?;
        let location = track.artwork.as_ref()?;
        let mut file = File::open(&track.path).ok()?;
        file.seek(SeekFrom::Start(location.offset)).ok()?;
        let mut bytes = vec![0; location.length];
        file.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }

    fn menu_theme_index(&self) -> Option<usize> {
        self.tracks
            .iter()
            .position(|track| track.number == MENU_THEME_NUMBER)
            .or_else(|| (!self.tracks.is_empty()).then_some(0))
    }

    fn begin_game_playlist(&mut self) {
        self.play_order = (0..self.tracks.len()).collect();
        if self.shuffle_music {
            shuffle_indices(&mut self.play_order);
        }
        self.play_position = 0;
        if let Some(&track_index) = self.play_order.first() {
            self.transition_to(track_index);
        }
    }

    fn advance_music(&mut self) {
        match self.music_mode {
            MusicMode::Menu => {
                if let Some(menu_theme) = self.menu_theme_index() {
                    self.transition_to(menu_theme);
                }
            }
            MusicMode::Game => {
                if self.play_order.is_empty() {
                    return;
                }
                self.play_position = (self.play_position + 1) % self.play_order.len();
                self.transition_to(self.play_order[self.play_position]);
            }
        }
    }

    fn transition_to(&mut self, track_index: usize) {
        let Some(voice) = self.start_voice(track_index) else {
            return;
        };
        if let Some(previous) = self.outgoing_music.take() {
            previous.sink.stop();
        }
        self.outgoing_music = self.active_music.take().map(|mut voice| {
            voice.fade_elapsed = 0.0;
            voice
        });
        self.active_music = Some(voice);
        self.refresh_music_volumes();
    }

    fn start_voice(&self, track_index: usize) -> Option<MusicVoice> {
        let handle = self.stream_handle.as_ref()?;
        let track = self.tracks.get(track_index)?;
        let file = File::open(&track.path).ok()?;
        let decoder = Decoder::new(BufReader::new(file)).ok()?;
        let duration = decoder
            .total_duration()
            .map(|duration| duration.as_secs_f32());
        let sink = Sink::try_new(handle).ok()?;
        sink.set_volume(0.0);
        sink.append(decoder);
        sink.play();
        Some(MusicVoice {
            sink,
            track_index,
            elapsed: 0.0,
            duration,
            fade_elapsed: 0.0,
        })
    }

    fn update_music(&mut self, dt: f32) {
        if let Some(active) = &mut self.active_music {
            active.elapsed += dt;
            active.fade_elapsed = (active.fade_elapsed + dt).min(MUSIC_CROSSFADE_SECONDS);
        }
        if let Some(outgoing) = &mut self.outgoing_music {
            outgoing.fade_elapsed += dt;
        }
        self.refresh_music_volumes();

        let outgoing_done = self.outgoing_music.as_ref().is_some_and(|voice| {
            voice.fade_elapsed >= MUSIC_CROSSFADE_SECONDS || voice.sink.empty()
        });
        if outgoing_done {
            if let Some(voice) = self.outgoing_music.take() {
                voice.sink.stop();
            }
        }

        let should_advance = self.active_music.as_ref().is_some_and(|voice| {
            voice.sink.empty()
                || voice.duration.is_some_and(|duration| {
                    voice.elapsed >= (duration - MUSIC_CROSSFADE_SECONDS).max(0.1)
                })
        });
        if should_advance {
            self.advance_music();
        }
    }

    fn refresh_music_volumes(&self) {
        if let Some(active) = &self.active_music {
            let fade = (active.fade_elapsed / MUSIC_CROSSFADE_SECONDS).clamp(0.0, 1.0);
            active.sink.set_volume(self.music_volume * fade);
        }
        if let Some(outgoing) = &self.outgoing_music {
            let fade = 1.0 - (outgoing.fade_elapsed / MUSIC_CROSSFADE_SECONDS).clamp(0.0, 1.0);
            outgoing.sink.set_volume(self.music_volume * fade);
        }
    }
}

fn music_directory() -> Option<PathBuf> {
    if let Ok(executable) = std::env::current_exe() {
        if let Some(contents) = executable.parent().and_then(Path::parent) {
            let bundled = contents.join("Resources/music");
            if bundled.is_dir() {
                return Some(bundled);
            }
        }
    }
    let local = PathBuf::from("music");
    local.is_dir().then_some(local)
}

fn discover_music_tracks() -> Vec<MusicTrack> {
    let Some(directory) = music_directory() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut tracks = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("mp3") {
            continue;
        }
        let Some(file_stem) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some((prefix, fallback_title)) = file_stem.split_once(" - ") else {
            continue;
        };
        let Ok(number) = prefix.parse::<u32>() else {
            continue;
        };
        let fallback_title = fallback_title.to_owned();
        let metadata = read_id3_metadata(&path).unwrap_or_default();
        tracks.push(MusicTrack {
            number,
            path,
            title: metadata.title.unwrap_or(fallback_title),
            artist: metadata.artist.unwrap_or_else(|| "ArtfulExpCHC".to_owned()),
            album: metadata
                .album
                .unwrap_or_else(|| "RIDGELINE SOUNDTRACK".to_owned()),
            artwork: metadata.artwork,
        });
    }
    tracks.sort_unstable_by_key(|track| track.number);
    tracks
}

fn shuffle_indices(indices: &mut [usize]) {
    let mut state = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0xA17F_91D3_62B4_CE05, |duration| duration.as_nanos() as u64);
    for index in (1..indices.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        indices.swap(index, state as usize % (index + 1));
    }
}

#[derive(Default)]
struct Id3Metadata {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    artwork: Option<ArtworkLocation>,
}

fn read_id3_metadata(path: &Path) -> Option<Id3Metadata> {
    let mut file = File::open(path).ok()?;
    let mut header = [0u8; 10];
    file.read_exact(&mut header).ok()?;
    if &header[0..3] != b"ID3" {
        return None;
    }
    let version = header[3];
    let tag_size = syncsafe_u32([header[6], header[7], header[8], header[9]]) as usize;
    let mut tag = vec![0; tag_size];
    file.read_exact(&mut tag).ok()?;

    let mut metadata = Id3Metadata::default();
    let mut cursor = 0usize;
    while cursor + 10 <= tag.len() {
        let id = &tag[cursor..cursor + 4];
        if id.iter().all(|byte| *byte == 0) {
            break;
        }
        let size_bytes = [
            tag[cursor + 4],
            tag[cursor + 5],
            tag[cursor + 6],
            tag[cursor + 7],
        ];
        let frame_size = if version >= 4 {
            syncsafe_u32(size_bytes) as usize
        } else {
            u32::from_be_bytes(size_bytes) as usize
        };
        let body_start = cursor + 10;
        let body_end = body_start.saturating_add(frame_size);
        if frame_size == 0 || body_end > tag.len() {
            break;
        }
        let body = &tag[body_start..body_end];
        match id {
            b"TIT2" => metadata.title = decode_id3_text(body),
            b"TPE1" => metadata.artist = decode_id3_text(body),
            b"TALB" => metadata.album = decode_id3_text(body),
            b"APIC" if metadata.artwork.is_none() => {
                if let Some(image_start) = apic_image_start(body) {
                    metadata.artwork = Some(ArtworkLocation {
                        offset: (10 + body_start + image_start) as u64,
                        length: body.len() - image_start,
                    });
                }
            }
            _ => {}
        }
        cursor = body_end;
    }
    Some(metadata)
}

fn syncsafe_u32(bytes: [u8; 4]) -> u32 {
    ((bytes[0] as u32 & 0x7f) << 21)
        | ((bytes[1] as u32 & 0x7f) << 14)
        | ((bytes[2] as u32 & 0x7f) << 7)
        | (bytes[3] as u32 & 0x7f)
}

fn decode_id3_text(body: &[u8]) -> Option<String> {
    let (&encoding, bytes) = body.split_first()?;
    let decoded = match encoding {
        0 => bytes
            .iter()
            .take_while(|byte| **byte != 0)
            .map(|byte| char::from(*byte))
            .collect(),
        3 => String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .to_owned(),
        1 | 2 => {
            let (little_endian, bytes) = if encoding == 1 && bytes.starts_with(&[0xff, 0xfe]) {
                (true, &bytes[2..])
            } else if encoding == 1 && bytes.starts_with(&[0xfe, 0xff]) {
                (false, &bytes[2..])
            } else {
                (false, bytes)
            };
            let words: Vec<_> = bytes
                .chunks_exact(2)
                .map(|pair| {
                    if little_endian {
                        u16::from_le_bytes([pair[0], pair[1]])
                    } else {
                        u16::from_be_bytes([pair[0], pair[1]])
                    }
                })
                .take_while(|word| *word != 0)
                .collect();
            String::from_utf16_lossy(&words)
        }
        _ => return None,
    };
    let decoded = decoded.trim().to_owned();
    (!decoded.is_empty()).then_some(decoded)
}

fn apic_image_start(body: &[u8]) -> Option<usize> {
    let encoding = *body.first()?;
    let mime_end = body[1..].iter().position(|byte| *byte == 0)? + 1;
    let mut cursor = mime_end + 2;
    if cursor >= body.len() {
        return None;
    }
    if matches!(encoding, 1 | 2) {
        while cursor + 1 < body.len() {
            if body[cursor] == 0 && body[cursor + 1] == 0 {
                return Some(cursor + 2);
            }
            cursor += 2;
        }
    } else if let Some(end) = body[cursor..].iter().position(|byte| *byte == 0) {
        return Some(cursor + end + 1);
    }
    None
}

struct ProceduralRolling {
    intensity: Arc<AtomicU32>,
    phase: f32,
    noise: u32,
}

impl ProceduralRolling {
    fn new(intensity: Arc<AtomicU32>) -> Self {
        Self {
            intensity,
            phase: 0.0,
            noise: 0x1234_5678,
        }
    }
}

impl Iterator for ProceduralRolling {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let speed = f32::from_bits(self.intensity.load(Ordering::Relaxed));
        let rate = 48_000.0;
        self.phase = (self.phase + (30.0 + speed * 54.0) / rate) % 1.0;
        self.noise = self.noise.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = ((self.noise >> 9) as f32 / (1u32 << 23) as f32) * 2.0 - 1.0;
        Some(((self.phase * std::f32::consts::TAU).sin() * 0.25 + noise * 0.10) * speed)
    }
}

impl Source for ProceduralRolling {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        1
    }

    fn sample_rate(&self) -> u32 {
        48_000
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct Chime {
    frequency: f32,
    samples_left: u32,
    total_samples: u32,
    phase: f32,
    gain: f32,
}

impl Chime {
    fn new(frequency: f32, duration: f32, gain: f32) -> Self {
        let total_samples = (48_000.0 * duration) as u32;
        Self {
            frequency,
            samples_left: total_samples,
            total_samples,
            phase: 0.0,
            gain,
        }
    }
}

impl Iterator for Chime {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.samples_left == 0 {
            return None;
        }
        let progress = 1.0 - self.samples_left as f32 / self.total_samples as f32;
        let envelope = (1.0 - progress).powf(2.2) * (progress * 40.0).min(1.0);
        self.phase = (self.phase + self.frequency * (1.0 + progress * 0.08) / 48_000.0) % 1.0;
        self.samples_left -= 1;
        Some((self.phase * std::f32::consts::TAU).sin() * envelope * self.gain)
    }
}

impl Source for Chime {
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.samples_left as usize)
    }

    fn channels(&self) -> u16 {
        1
    }

    fn sample_rate(&self) -> u32 {
        48_000
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(
            self.total_samples as f32 / 48_000.0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundtrack_is_numerically_ordered_and_contains_the_menu_theme() {
        let tracks = discover_music_tracks();
        assert_eq!(tracks.len(), 38);
        assert!(tracks
            .windows(2)
            .all(|pair| pair[0].number < pair[1].number));
        let menu = tracks
            .iter()
            .find(|track| track.number == MENU_THEME_NUMBER)
            .expect("numbered menu theme");
        assert_eq!(menu.title, "Haunted Heartbeats 1.1");
        assert!(!menu.artist.is_empty());
        assert!(menu.artwork.is_some());
    }

    #[test]
    fn shuffle_preserves_every_track_index() {
        let mut indices: Vec<_> = (0..38).collect();
        shuffle_indices(&mut indices);
        indices.sort_unstable();
        assert_eq!(indices, (0..38).collect::<Vec<_>>());
    }
}
