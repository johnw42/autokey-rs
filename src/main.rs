use config::Config;
use display::{Display, KeyboardMapping, RecordedEvent};
use enumset::EnumSet;
use libc::c_int;
use log::{debug, info};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::convert::TryFrom;

use x11::xlib::{ButtonPress, KeyPress, KeyRelease, XEvent};
use x11::xlib::{CreateNotify, Window};

mod config;
mod display;
mod key;

use key::*;

use crate::config::KeySpec;
use crate::display::RecordingDisplay;

struct AppState<'d> {
    display: &'d Display,
    _keys_down: BTreeSet<Keycode>,
    config: Config,
    keyboard_mapping: KeyboardMapping,
    _modifiers: EnumSet<Modifier>,
}

impl<'d> AppState<'d> {
    fn _keysym_to_keycode(&self, keysym: Keysym) -> Option<Keycode> {
        self.keyboard_mapping
            .keysym_to_keycode
            .get(&keysym)
            .copied()
    }

    fn get_keyboard_mapping(&mut self) {
        self.keyboard_mapping = self.display.get_keyboard_mapping();
    }

    fn keycode_to_keysym(&self, keycode: Keycode) -> Option<Keysym> {
        self.keyboard_mapping
            .keycode_to_keysyms
            .get(&keycode)
            .map(|v| v[0])
    }

    fn keycode_to_string(&self, keycode: Keycode) -> String {
        self.keycode_to_keysym(keycode)
            .and_then(|k| k.to_string())
            .map(|s| format!("<{}>", s))
            .unwrap_or_else(|| format!("<keycode_{}>", keycode.value()))
    }

    fn log_key(&self, label: &str, keycode: Keycode, state: u16) {
        debug!(
            "{}: code={}, sym={} ({:?}), state={}, down=[{}]",
            label,
            keycode.value(),
            self.keycode_to_keysym(keycode)
                .map(|k| k.value())
                .unwrap_or(0),
            self.keycode_to_string(keycode),
            state,
            self._keys_down
                .iter()
                .map(|&k| self.keycode_to_string(k))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    fn handle_recorded_event(&mut self, event: &RecordedEvent) {
        #[allow(non_upper_case_globals)]
        match event.code as c_int {
            KeyPress => {
                if let Ok(code) = Keycode::try_from(event.detail) {
                    self._keys_down.insert(code);
                    self.log_key("KeyPress", code, event.state);
                }
            }
            KeyRelease => {
                if let Ok(code) = Keycode::try_from(event.detail) {
                    self._keys_down.remove(&code);
                    self.log_key("KeyRelease", code, event.state);
                    // if code == 52 && event.state == 0xc {
                    //     let press = 1;
                    //     let release = 0;
                    //     unsafe {
                    //         for &key in self.keys_down.iter() {
                    //             XTestFakeKeyEvent(
                    //                 self.main_display,
                    //                 key.value() as c_uint,
                    //                 release,
                    //                 0,
                    //             );
                    //         }
                    //         XTestFakeKeyEvent(self.main_display, 52, press, 0);
                    //         XTestFakeKeyEvent(self.main_display, 52, release, 0);
                    //         for &key in self.keys_down.iter() {
                    //             XTestFakeKeyEvent(
                    //                 self.main_display,
                    //                 key.value() as c_uint,
                    //                 press,
                    //                 0,
                    //             );
                    //         }
                    //     }
                    // }
                }
            }
            ButtonPress => {
                println!("ButtonPress: {} {:x}", event.detail, event.state);
            }
            _ => {
                println!("event type: {:?}", event);
            }
        }
    }

    fn grab_keys_for_window(&mut self, window: Window) {
        self.display.visit_window_tree(window, &mut |child| {
            self.config.visit_key_mappings(&|k| match k.input {
                KeySpec::Code(c) => {
                    self.display.grab_key(
                        child,
                        Keycode::try_from(c as u8).expect("invalid keycode"),
                        Some(EnumSet::<Modifier>::from_u8(0xc)),
                    );
                }
                KeySpec::Sym(_) => {}
            })
        });
    }

    fn handle_xevent(&mut self, event: XEvent) {
        unsafe {
            #[allow(non_upper_case_globals)]
            match event.any.type_ as c_int {
                CreateNotify => {
                    let event = event.create_window;
                    self.grab_keys_for_window(event.window);
                }
                _ => {}
            }
        }
    }

    fn run() {
        let display = Display::new();

        let config = json5::from_str(include_str!("config.json5")).unwrap();
        info!("config: {:?}", config);
        let keyboard_mapping = display.get_keyboard_mapping();

        let mut state = AppState {
            display: &display,
            _keys_down: Default::default(),
            config,
            keyboard_mapping,
            _modifiers: Default::default(),
        };
        state.get_keyboard_mapping();

        // config.visit_keyspecs(|k| match k {
        //     KeySpec::Code(_) => {}
        //     KeySpec::Sym(s) => {
        //         *k = KeySpec::Code(
        //             app_state
        //                 .keysym_to_keycode(s.parse().expect("invalid key name"))
        //                 .expect("invalid keysym")
        //                 .value() as i32,
        //         );
        //     }
        // });

        let state = RefCell::new(state);
        let record_display =
            RecordingDisplay::new(|event| state.borrow_mut().handle_recorded_event(event));
        display.event_loop(&record_display, |event| {
            state.borrow_mut().handle_xevent(event)
        })
    }
}

fn main() {
    env_logger::init();
    AppState::run();
}
