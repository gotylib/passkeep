use eframe::egui;

/// Windows reports AltGr as Ctrl+Alt, and egui 0.33 then drops the typed character.
/// `@` and the other symbols on that chord never reach the field, so the master
/// password does not match what was typed. Ask Windows for the real character
/// and put it back.
pub fn recover_special_keys(raw: &mut egui::RawInput) {
    #[cfg(windows)]
    windows::recover(raw);
    #[cfg(not(windows))]
    let _ = raw;
}

#[cfg(windows)]
mod windows {
    use super::egui;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, MapVirtualKeyW, ToUnicodeEx, MAPVK_VK_TO_VSC, VK_CONTROL, VK_LSHIFT,
        VK_MENU, VK_RMENU, VK_SHIFT,
    };

    const DONT_CHANGE_KEYBOARD_STATE: u32 = 4;

    pub fn recover(raw: &mut egui::RawInput) {
        let mut index = 0;
        while index < raw.events.len() {
            let replacement = match &raw.events[index] {
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.ctrl && modifiers.alt => {
                    special_character(physical_key.unwrap_or(*key), modifiers.shift)
                        .map(Replacement::AfterKey)
                }
                egui::Event::Copy if raw.modifiers.ctrl && raw.modifiers.alt => {
                    special_character(egui::Key::C, raw.modifiers.shift).map(Replacement::Instead)
                }
                egui::Event::Cut if raw.modifiers.ctrl && raw.modifiers.alt => {
                    special_character(egui::Key::X, raw.modifiers.shift).map(Replacement::Instead)
                }
                egui::Event::Paste(_) if raw.modifiers.ctrl && raw.modifiers.alt => {
                    special_character(egui::Key::V, raw.modifiers.shift).map(Replacement::Instead)
                }
                _ => None,
            };

            match replacement {
                Some(Replacement::AfterKey(text)) => {
                    if let egui::Event::Key { modifiers, .. } = &mut raw.events[index] {
                        modifiers.ctrl = false;
                        modifiers.command = false;
                        modifiers.mac_cmd = false;
                    }
                    let already_there = matches!(
                        raw.events.get(index + 1),
                        Some(egui::Event::Text(existing)) if existing == &text
                    );
                    if !already_there {
                        raw.events.insert(index + 1, egui::Event::Text(text));
                        index += 1;
                    }
                }
                Some(Replacement::Instead(text)) => {
                    raw.events[index] = egui::Event::Text(text);
                }
                None => {}
            }
            index += 1;
        }
    }

    enum Replacement {
        AfterKey(String),
        Instead(String),
    }

    #[cfg(test)]
    thread_local! {
        static FORCED_ALTGR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    }

    fn special_character(key: egui::Key, shift: bool) -> Option<String> {
        #[cfg(test)]
        if let Some(text) = FORCED_ALTGR.with(|slot| slot.borrow().clone()) {
            return Some(text);
        }
        let vk = virtual_key(key)?;
        let layout = unsafe { GetKeyboardLayout(0) };
        let with_altgr = translate(vk, &chord(vk, shift, true), layout)?;
        let without_altgr = translate(vk, &chord(vk, shift, false), layout);
        if without_altgr.as_deref() == Some(with_altgr.as_str()) {
            return None;
        }
        if with_altgr.chars().any(char::is_control) {
            return None;
        }
        Some(with_altgr)
    }

    fn translate(
        vk: u16,
        state: &[u8; 256],
        layout: windows_sys::Win32::UI::Input::KeyboardAndMouse::HKL,
    ) -> Option<String> {
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };
        let mut buffer = [0u16; 8];
        let written = unsafe {
            ToUnicodeEx(
                vk as u32,
                scan,
                state.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as i32,
                DONT_CHANGE_KEYBOARD_STATE,
                layout,
            )
        };
        if written <= 0 {
            return None;
        }
        let text = String::from_utf16_lossy(&buffer[..written as usize]);
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn chord(vk: u16, shift: bool, altgr: bool) -> [u8; 256] {
        let mut state = [0u8; 256];
        state[vk as usize] |= 0x80;
        if shift {
            state[VK_SHIFT as usize] |= 0x80;
            state[VK_LSHIFT as usize] |= 0x80;
        }
        if altgr {
            // Right Alt is AltGr. Left Ctrl must stay up: with it down, Windows
            // treats the chord as a shortcut and returns no character.
            state[VK_CONTROL as usize] |= 0x80;
            state[VK_MENU as usize] |= 0x80;
            state[VK_RMENU as usize] |= 0x80;
        }
        state
    }

    fn virtual_key(key: egui::Key) -> Option<u16> {
        use egui::Key::{
            Backslash, Backtick, CloseBracket, CloseCurlyBracket, Colon, Comma, Equals, Minus,
            Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9, OpenBracket,
            OpenCurlyBracket, Period, Pipe, Plus, Questionmark, Quote, Semicolon, Slash, Space, A,
            B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
        };
        Some(match key {
            A => b'A' as u16,
            B => b'B' as u16,
            C => b'C' as u16,
            D => b'D' as u16,
            E => b'E' as u16,
            F => b'F' as u16,
            G => b'G' as u16,
            H => b'H' as u16,
            I => b'I' as u16,
            J => b'J' as u16,
            K => b'K' as u16,
            L => b'L' as u16,
            M => b'M' as u16,
            N => b'N' as u16,
            O => b'O' as u16,
            P => b'P' as u16,
            Q => b'Q' as u16,
            R => b'R' as u16,
            S => b'S' as u16,
            T => b'T' as u16,
            U => b'U' as u16,
            V => b'V' as u16,
            W => b'W' as u16,
            X => b'X' as u16,
            Y => b'Y' as u16,
            Z => b'Z' as u16,
            Num0 => b'0' as u16,
            Num1 => b'1' as u16,
            Num2 => b'2' as u16,
            Num3 => b'3' as u16,
            Num4 => b'4' as u16,
            Num5 => b'5' as u16,
            Num6 => b'6' as u16,
            Num7 => b'7' as u16,
            Num8 => b'8' as u16,
            Num9 => b'9' as u16,
            Minus => 0xBD,
            Plus | Equals => 0xBB,
            OpenBracket | OpenCurlyBracket => 0xDB,
            CloseBracket | CloseCurlyBracket => 0xDD,
            Backslash | Pipe => 0xDC,
            Semicolon | Colon => 0xBA,
            Quote => 0xDE,
            Comma => 0xBC,
            Period => 0xBE,
            Slash | Questionmark => 0xBF,
            Backtick => 0xC0,
            Space => 0x20,
            _ => return None,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::{recover, virtual_key, FORCED_ALTGR};
        use eframe::egui::{Event, Key, Modifiers, RawInput};
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            ActivateKeyboardLayout, GetKeyboardLayout, LoadKeyboardLayoutW, UnloadKeyboardLayout,
            KLF_ACTIVATE, KLF_NOTELLSHELL,
        };

        struct LayoutGuard {
            previous: windows_sys::Win32::UI::Input::KeyboardAndMouse::HKL,
            loaded: windows_sys::Win32::UI::Input::KeyboardAndMouse::HKL,
        }

        impl Drop for LayoutGuard {
            fn drop(&mut self) {
                unsafe {
                    ActivateKeyboardLayout(self.previous, KLF_NOTELLSHELL);
                    UnloadKeyboardLayout(self.loaded);
                }
            }
        }

        /// Only for CI. Loading a layout here adds it to the signed-in Windows
        /// session, which is why extra layouts showed up in the switcher.
        fn use_layout(klid: &str) -> LayoutGuard {
            assert_eq!(
                std::env::var("PASSKEEP_LAYOUT_TESTS").ok().as_deref(),
                Some("1"),
                "refusing to load {klid} into the Windows session"
            );
            let previous = unsafe { GetKeyboardLayout(0) };
            let wide: Vec<u16> = klid.encode_utf16().chain(std::iter::once(0)).collect();
            let loaded =
                unsafe { LoadKeyboardLayoutW(wide.as_ptr(), KLF_ACTIVATE | KLF_NOTELLSHELL) };
            assert!(!loaded.is_null(), "keyboard layout {klid} did not load");
            LayoutGuard { previous, loaded }
        }

        fn layout_tests_enabled() -> bool {
            std::env::var("PASSKEEP_LAYOUT_TESTS").ok().as_deref() == Some("1")
        }

        fn altgr_key(key: Key, shift: bool) -> Event {
            Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: Modifiers {
                    alt: true,
                    ctrl: true,
                    shift,
                    command: true,
                    mac_cmd: false,
                },
            }
        }

        #[test]
        fn german_altgr_q_is_inserted_as_at() {
            if !layout_tests_enabled() {
                return;
            }
            let _guard = use_layout("00000407");
            let mut raw = RawInput::default();
            raw.events.push(altgr_key(Key::Q, false));
            recover(&mut raw);
            assert!(
                matches!(raw.events.get(1), Some(Event::Text(text)) if text == "@"),
                "events: {:?}",
                raw.events
            );
            match &raw.events[0] {
                Event::Key { modifiers, .. } => {
                    assert!(!modifiers.ctrl);
                    assert!(!modifiers.command);
                }
                other => panic!("expected the key event, got {other:?}"),
            }
        }

        #[test]
        fn spanish_altgr_2_is_inserted_as_at() {
            if !layout_tests_enabled() {
                return;
            }
            let _guard = use_layout("0000040A");
            let mut raw = RawInput::default();
            raw.events.push(altgr_key(Key::Num2, false));
            recover(&mut raw);
            assert!(
                matches!(raw.events.get(1), Some(Event::Text(text)) if text == "@"),
                "events: {:?}",
                raw.events
            );
        }

        #[test]
        fn ctrl_without_alt_is_not_rewritten() {
            let mut raw = RawInput::default();
            raw.events.push(Event::Key {
                key: Key::Q,
                physical_key: Some(Key::Q),
                pressed: true,
                repeat: false,
                modifiers: Modifiers {
                    alt: false,
                    ctrl: true,
                    shift: false,
                    command: true,
                    mac_cmd: false,
                },
            });
            recover(&mut raw);
            assert_eq!(raw.events.len(), 1);
            assert!(virtual_key(Key::Q).is_some());
        }

        #[test]
        fn altgr_text_is_inserted_without_loading_a_layout() {
            FORCED_ALTGR.with(|slot| *slot.borrow_mut() = Some("@".to_string()));
            struct Clear;
            impl Drop for Clear {
                fn drop(&mut self) {
                    FORCED_ALTGR.with(|slot| *slot.borrow_mut() = None);
                }
            }
            let _clear = Clear;
            let mut raw = RawInput::default();
            raw.events.push(altgr_key(Key::Q, false));
            recover(&mut raw);
            assert!(matches!(raw.events.get(1), Some(Event::Text(text)) if text == "@"));
            match &raw.events[0] {
                Event::Key { modifiers, .. } => {
                    assert!(!modifiers.ctrl);
                    assert!(!modifiers.command);
                }
                other => panic!("expected the key event, got {other:?}"),
            }
        }

        #[test]
        fn released_altgr_key_is_not_rewritten() {
            let mut raw = RawInput::default();
            raw.events.push(Event::Key {
                key: Key::Q,
                physical_key: Some(Key::Q),
                pressed: false,
                repeat: false,
                modifiers: Modifiers {
                    alt: true,
                    ctrl: true,
                    shift: false,
                    command: true,
                    mac_cmd: false,
                },
            });
            recover(&mut raw);
            assert_eq!(raw.events.len(), 1);
        }
    }
}
