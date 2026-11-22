use autopilot::geometry::Size;
use autopilot::mouse;
use autopilot::mouse::ScrollDirection;
use autopilot::screen::size as screen_size;

use tracing::warn;

use crate::input::device::{InputDevice, InputDeviceType};
use crate::protocol::{Button, KeyboardEvent, KeyboardEventType, PointerEvent, WheelEvent};

use crate::capturable::{Capturable, Geometry};

pub struct AutoPilotDevice {
    capturable: Box<dyn Capturable>,
}

impl AutoPilotDevice {
    pub fn new(capturable: Box<dyn Capturable>) -> Self {
        Self { capturable }
    }
}

impl InputDevice for AutoPilotDevice {
    fn send_wheel_event(&mut self, event: &WheelEvent) {
        match event.dy {
            1..=i32::MAX => mouse::scroll(ScrollDirection::Up, 1),
            i32::MIN..=-1 => mouse::scroll(ScrollDirection::Down, 1),
            0 => {}
        }
    }

    fn send_pointer_event(&mut self, event: &PointerEvent) {
        if !event.is_primary {
            return;
        }
        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }
        let (x_rel, y_rel, width_rel, height_rel) = match self.capturable.geometry().unwrap() {
            Geometry::Relative(x, y, width, height) => (x, y, width, height),
            #[cfg(target_os = "windows")]
            _ => {
                warn!("Failed to get window geometry, sending no input");
                return;
            }
        };
        #[cfg(not(target_os = "macos"))]
        let Size { width, height } = screen_size();
        #[cfg(target_os = "macos")]
        let (_, _, width, height) = match crate::capturable::core_graphics::screen_coordsys() {
            Ok(bounds) => bounds,
            Err(err) => {
                warn!("Could not determine global coordinate system: {}", err);
                return;
            }
        };
        if let Err(err) = mouse::move_to(autopilot::geometry::Point::new(
            (event.x * width_rel + x_rel) * width,
            (event.y * height_rel + y_rel) * height,
        )) {
            warn!("Could not move mouse: {}", err);
        }
        match event.button {
            Button::PRIMARY => {
                mouse::toggle(mouse::Button::Left, event.buttons.contains(event.button))
            }
            Button::AUXILARY => {
                mouse::toggle(mouse::Button::Middle, event.buttons.contains(event.button))
            }
            Button::SECONDARY => {
                mouse::toggle(mouse::Button::Right, event.buttons.contains(event.button))
            }
            _ => (),
        }
    }

    fn send_keyboard_event(&mut self, event: &KeyboardEvent) {
        use autopilot::key::{Character, Code, KeyCode};

        let state = match event.event_type {
            KeyboardEventType::UP => false,
            KeyboardEventType::DOWN => true,
            // autopilot doesn't handle this, so just do nothing
            KeyboardEventType::REPEAT => return,
        };

        fn map_key(code: &str) -> Option<KeyCode> {
            match code {
                "Escape" => Some(KeyCode::Escape),
                "Enter" => Some(KeyCode::Return),
                "Backspace" => Some(KeyCode::Backspace),
                "Tab" => Some(KeyCode::Tab),
                "Space" => Some(KeyCode::Space),
                "CapsLock" => Some(KeyCode::CapsLock),
                "F1" => Some(KeyCode::F1),
                "F2" => Some(KeyCode::F2),
                "F3" => Some(KeyCode::F3),
                "F4" => Some(KeyCode::F4),
                "F5" => Some(KeyCode::F5),
                "F6" => Some(KeyCode::F6),
                "F7" => Some(KeyCode::F7),
                "F8" => Some(KeyCode::F8),
                "F9" => Some(KeyCode::F9),
                "F10" => Some(KeyCode::F10),
                "F11" => Some(KeyCode::F11),
                "F12" => Some(KeyCode::F12),
                "F13" => Some(KeyCode::F13),
                "F14" => Some(KeyCode::F14),
                "F15" => Some(KeyCode::F15),
                "F16" => Some(KeyCode::F16),
                "F17" => Some(KeyCode::F17),
                "F18" => Some(KeyCode::F18),
                "F19" => Some(KeyCode::F19),
                "F20" => Some(KeyCode::F20),
                "F21" => Some(KeyCode::F21),
                "F22" => Some(KeyCode::F22),
                "F23" => Some(KeyCode::F23),
                "F24" => Some(KeyCode::F24),
                "Home" => Some(KeyCode::Home),
                "ArrowUp" => Some(KeyCode::UpArrow),
                "PageUp" => Some(KeyCode::PageUp),
                "ArrowLeft" => Some(KeyCode::LeftArrow),
                "ArrowRight" => Some(KeyCode::RightArrow),
                "End" => Some(KeyCode::End),
                "ArrowDown" => Some(KeyCode::DownArrow),
                "PageDown" => Some(KeyCode::PageDown),
                "Delete" => Some(KeyCode::Delete),
                "ControlLeft" | "ControlRight" => Some(KeyCode::Control),
                "AltLeft" | "AltRight" => Some(KeyCode::Alt),
                "MetaLeft" | "MetaRight" => Some(KeyCode::Meta),
                "ShiftLeft" | "ShiftRight" => Some(KeyCode::Shift),
                _ => None,
            }
        }
        let key = map_key(&event.code);
        let mut flags = Vec::new();
        if event.ctrl {
            flags.push(autopilot::key::Flag::Control);
        }
        if event.alt {
            flags.push(autopilot::key::Flag::Alt);
        }
        if event.meta {
            flags.push(autopilot::key::Flag::Meta);
        }
        if event.shift {
            flags.push(autopilot::key::Flag::Shift);
        }
        match key {
            Some(key) => autopilot::key::toggle(&Code(key), state, &flags, 0),
            None => {
                for c in event.key.chars() {
                    // autopilot's Windows character path panics (transmute_copy into the larger INPUT union)
                    #[cfg(target_os = "windows")]
                    win_type_char(c, state, event.ctrl || event.alt || event.meta, &flags);
                    #[cfg(not(target_os = "windows"))]
                    autopilot::key::toggle(&Character(c), state, &flags, 0);
                }
            }
        }
    }

    fn set_capturable(&mut self, capturable: Box<dyn Capturable>) {
        self.capturable = capturable;
    }

    fn device_type(&self) -> InputDeviceType {
        InputDeviceType::AutoPilotDevice
    }
}

/// Types one character on Windows. With Ctrl/Alt/Win held it presses the matching virtual key so
/// shortcuts like Ctrl+C work; otherwise it sends the character as Unicode (any layout, any symbol).
#[cfg(target_os = "windows")]
fn win_type_char(c: char, down: bool, shortcut: bool, flags: &[autopilot::key::Flag]) {
    use autopilot::key::Flag;
    use winapi::um::winuser::{
        keybd_event, SendInput, VkKeyScanW, INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT,
    };
    let up = if down { 0 } else { KEYEVENTF_KEYUP };
    let mut buf = [0u16; 2];
    let words = c.encode_utf16(&mut buf);
    unsafe {
        if shortcut && words.len() == 1 && (VkKeyScanW(words[0]) as u16 & 0xff) != 0xff {
            let mods: Vec<u8> = flags
                .iter()
                .map(|f| match f {
                    Flag::Control => VK_CONTROL as u8,
                    Flag::Alt => VK_MENU as u8,
                    Flag::Meta => VK_LWIN as u8,
                    Flag::Shift => VK_SHIFT as u8,
                    _ => 0,
                })
                .filter(|v| *v != 0)
                .collect();
            let vk = (VkKeyScanW(words[0]) as u16 & 0xff) as u8;
            if down {
                for m in &mods {
                    keybd_event(*m, 0, 0, 0);
                }
                keybd_event(vk, 0, 0, 0);
            } else {
                keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
                for m in mods.iter().rev() {
                    keybd_event(*m, 0, KEYEVENTF_KEYUP, 0);
                }
            }
            return;
        }
        for w in words.iter() {
            let mut input: INPUT = std::mem::zeroed();
            input.type_ = INPUT_KEYBOARD;
            let ki = input.u.ki_mut();
            ki.wScan = *w;
            ki.dwFlags = KEYEVENTF_UNICODE | up;
            SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32);
        }
    }
}