use gilrs::{Axis, Button, EventType, Gilrs};
use glam::Vec2;
use std::collections::HashSet;
use winit::{event::ElementState, keyboard::KeyCode};

pub struct InputState {
    keys: HashSet<KeyCode>,
    gamepad_move: Vec2,
    jump_pressed: bool,
    pause_pressed: bool,
    gilrs: Option<Gilrs>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            keys: HashSet::new(),
            gamepad_move: Vec2::ZERO,
            jump_pressed: false,
            pause_pressed: false,
            gilrs: Gilrs::new().ok(),
        }
    }

    pub fn keyboard(&mut self, code: KeyCode, state: ElementState, repeat: bool) {
        match state {
            ElementState::Pressed => {
                self.keys.insert(code);
                if code == KeyCode::Escape && !repeat {
                    self.pause_pressed = true;
                }
                if code == KeyCode::Space && !repeat {
                    self.jump_pressed = true;
                }
            }
            ElementState::Released => {
                self.keys.remove(&code);
            }
        }
    }

    pub fn poll_gamepad(&mut self) {
        let Some(gilrs) = self.gilrs.as_mut() else {
            return;
        };
        while let Some(event) = gilrs.next_event() {
            match event.event {
                EventType::AxisChanged(Axis::LeftStickX, value, _) => self.gamepad_move.x = value,
                EventType::AxisChanged(Axis::LeftStickY, value, _) => self.gamepad_move.y = value,
                EventType::ButtonPressed(Button::South, _) => self.jump_pressed = true,
                EventType::ButtonPressed(Button::Start, _) => self.pause_pressed = true,
                _ => {}
            }
        }
    }

    pub fn movement(&self) -> Vec2 {
        let mut keyboard = Vec2::ZERO;
        if self.keys.contains(&KeyCode::KeyA) || self.keys.contains(&KeyCode::ArrowLeft) {
            keyboard.x -= 1.0;
        }
        if self.keys.contains(&KeyCode::KeyD) || self.keys.contains(&KeyCode::ArrowRight) {
            keyboard.x += 1.0;
        }
        if self.keys.contains(&KeyCode::KeyW) || self.keys.contains(&KeyCode::ArrowUp) {
            keyboard.y += 1.0;
        }
        if self.keys.contains(&KeyCode::KeyS) || self.keys.contains(&KeyCode::ArrowDown) {
            keyboard.y -= 1.0;
        }
        if keyboard.length_squared() > 1.0 {
            keyboard = keyboard.normalize();
        }
        let gamepad =
            apply_radial_deadzone(Vec2::new(self.gamepad_move.x, -self.gamepad_move.y), 0.16);
        if gamepad.length_squared() > keyboard.length_squared() {
            gamepad
        } else {
            keyboard
        }
    }

    pub fn take_jump(&mut self) -> bool {
        std::mem::take(&mut self.jump_pressed)
    }

    pub fn take_pause(&mut self) -> bool {
        std::mem::take(&mut self.pause_pressed)
    }
}

fn apply_radial_deadzone(value: Vec2, deadzone: f32) -> Vec2 {
    let length = value.length();
    if length <= deadzone {
        Vec2::ZERO
    } else {
        value.normalize_or_zero() * ((length - deadzone) / (1.0 - deadzone)).clamp(0.0, 1.0)
    }
}
