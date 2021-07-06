mod config;
mod display;
mod key;

use config::{Config, KeySpec};
use display::{
    Display, Event, KeyboardMapping, RecordedEvent, RecordedEventDetail, RecordingDisplay, Window,
};
use enumset::EnumSet;
use key::*;
use log::{debug, info};
use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::convert::TryFrom;

struct AppState<'d> {
    display: &'d Display,
    _keys_down: BTreeSet<Keycode>,
    config: Config,
    keyboard_mapping: KeyboardMapping,
    _modifiers: EnumSet<Modifier>,
    ignore_queue: VecDeque<Box<dyn Fn(&RecordedEvent) -> bool + 'static>>,
}

impl<'d> AppState<'d> {
    fn _keysym_to_keycode(&self, keysym: Keysym) -> Option<Keycode> {
        self.keyboard_mapping
            .keysym_to_keycode
            .get(&keysym)
            .copied()
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

    fn log_key(&self, label: &str, keycode: Keycode, state: EnumSet<Modifier>) {
        debug!(
            "{}: code={}, sym={} ({:?}), state={:?}, down=[{}]",
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

    fn handle_recorded_event(&mut self, event: RecordedEvent) {
        if let Some(f) = self.ignore_queue.front() {
            if f(&event) {
                info!("ignoring event");
                self.ignore_queue.pop_front();
                return;
            }
        }

        match event.detail {
            RecordedEventDetail::KeyPress(code) => {
                // if code.value() == 15 {
                //     self.ignore_queue
                //         .push_back(Box::new(move |e| match &e.detail {
                //             RecordedEventDetail::KeyPress(c) if *c == code => true,
                //             _ => false,
                //         }));
                //     self.ignore_queue
                //         .push_back(Box::new(move |e| match &e.detail {
                //             RecordedEventDetail::KeyRelease(c) if *c == code => true,
                //             _ => false,
                //         }));
                //     self.display.send_key_event(code, display::KeyEvent::Press);
                //     self.display
                //         .send_key_event(code, display::KeyEvent::Release);
                //     info!("sent keycode");
                // }

                self._keys_down.insert(code);
                self.log_key("KeyPress", code, event.state);
            }
            RecordedEventDetail::KeyRelease(code) => {
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
            RecordedEventDetail::ButtonPress(button) => {
                println!("ButtonPress: {} {:?}", button, event.state);
            }
            RecordedEventDetail::Unknown { .. } => {}
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

    fn handle_xevent(&mut self, event: Event) {
        match event {
            Event::CreateNotify { window } => self.grab_keys_for_window(window),
        }
    }

    fn run() {
        let display = Display::new();

        // let keycode = Keycode::try_from(15).unwrap();
        // display.visit_window_tree(display.root_window(), &mut |window| {
        //     display.grab_key(window, keycode, Some(Default::default()));
        // });
        // return;

        let config = json5::from_str(include_str!("config.json5")).unwrap();
        info!("config: {:?}", config);
        let keyboard_mapping = display.get_keyboard_mapping();

        let state = AppState {
            display: &display,
            _keys_down: Default::default(),
            config,
            keyboard_mapping,
            _modifiers: Default::default(),
            ignore_queue: Default::default(),
        };

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
