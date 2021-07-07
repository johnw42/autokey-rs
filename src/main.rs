mod config;
mod display;
mod key;
mod key_grabber;

use config::{Config, ControlFlow, KeySpec};
use display::{
    Button, Display, Event, InputEvent, RecordedEvent, RecordingDisplay, UpOrDown, WindowRef,
};
use enumset::EnumSet;
use key::*;
use key_grabber::KeyGrabber;
use log::{debug, error, info};
use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::convert::TryFrom;

struct AppState {
    display: Display,
    keys_down: BTreeSet<Keycode>,
    config: Config,
    keyboard_mapping: KeyboardMapping,
    modifier_mapping: ModifierMapping,
    modifiers: EnumSet<Modifier>,
    ignore_queue: VecDeque<InputEvent>,
    grabber: KeyGrabber,
}

impl AppState {
    fn _keycode_to_string(&self, keycode: Keycode) -> String {
        self.keyboard_mapping
            ._keycode_to_keysyms(keycode)
            .get(0)
            .and_then(|k| k.to_string())
            .map(|s| format!("<{}>", s))
            .unwrap_or_else(|| format!("<keycode_{}>", keycode.value()))
    }

    fn _log_key(&self, label: &str, keycode: Keycode, state: EnumSet<Modifier>) {
        debug!(
            "{}: code={}, sym={} ({:?}), state={:?}, down=[{}]",
            label,
            keycode.value(),
            self.keyboard_mapping
                ._keycode_to_keysyms(keycode)
                .get(0)
                .map(|k| k.value())
                .unwrap_or(0),
            self._keycode_to_string(keycode),
            state,
            self.keys_down
                .iter()
                .map(|&k| self._keycode_to_string(k))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    fn send_input_event(&mut self, event: InputEvent) {
        info!("sending event: {:?}", event);
        match self.display.send_input_event(event.clone()) {
            Ok(_) => self.ignore_queue.push_back(event),
            Err(_) => error!("error sending input event: {:?}", event),
        }
    }

    fn keyspec_matches_button(&self, spec: &KeySpec, button: &Button) -> bool {
        let spec_code = match spec {
            KeySpec::Code(code) => Keycode::try_from(*code).ok(),
            KeySpec::Sym(sym) => sym
                .parse()
                .ok()
                .and_then(|sym| self.keyboard_mapping.keysym_to_keycode(sym)),
        };
        match button {
            Button::Key(button_code) => Some(*button_code) == spec_code,
            Button::MouseButton(_) => false,
        }
    }

    fn keyspec_to_input_event(&self, spec: &KeySpec, direction: UpOrDown) -> Option<InputEvent> {
        let code = match spec {
            KeySpec::Code(code) => Keycode::try_from(*code).ok(),
            KeySpec::Sym(sym) => sym
                .parse()
                .ok()
                .and_then(|sym| self.keyboard_mapping.keysym_to_keycode(sym)),
        };
        code.map(|code| InputEvent {
            button: Button::Key(code),
            direction,
        })
    }

    fn handle_recorded_event(&mut self, event: RecordedEvent) {
        use Button::*;
        use UpOrDown::*;

        info!("handling input event: {:?}", event);

        match event.input {
            InputEvent {
                direction,
                button: Key(code),
            } => match direction {
                Up => {
                    self.keys_down.remove(&code);
                }
                Down => {
                    self.keys_down.insert(code);
                }
            },
            _ => {}
        }

        self.modifiers = self
            .keys_down
            .iter()
            .map(|&keycode| self.modifier_mapping.keycode_to_modifiers(keycode))
            .collect();

        if let Some(to_ignore) = self.ignore_queue.front() {
            if event.input == *to_ignore {
                self.ignore_queue.pop_front();
                debug!(
                    "ignoring event {:?}, queue length: {}",
                    event,
                    self.ignore_queue.len()
                );
                return;
            }
        }

        match &event.input {
            InputEvent {
                direction: Up,
                button,
            } => {
                let mut to_send = Vec::new();
                self.config.visit_key_mappings(&mut |m| {
                    if self.keyspec_matches_button(&m.input, button)
                        && m.mods.matches(self.modifiers)
                    {
                        let output_seq = match &m.output {
                            config::KeySeq::Key(k) => vec![vec![k.clone()]],
                            config::KeySeq::Chord(c) => vec![c.clone()],
                            config::KeySeq::ChordSeq(s) => s.clone(),
                        };
                        for chord in output_seq {
                            let events = chord
                                .iter()
                                .map(|key| self.keyspec_to_input_event(key, Down))
                                .collect::<Option<Vec<_>>>();
                            match events {
                                Some(events) => {
                                    to_send.extend(events.iter().map(|e| e.clone()));
                                    to_send.extend(events.into_iter().rev().map(|mut e| {
                                        e.direction = Up;
                                        e
                                    }))
                                }
                                None => error!("invalid keyspec: {:?}", m.output),
                            }
                        }
                        ControlFlow::Break
                    } else {
                        ControlFlow::Continue
                    }
                });
                self.display.sync();
                self.grabber.push_state();
                for event in to_send.into_iter() {
                    if let Button::Key(keycode) = event.button {
                        self.grabber.ungrab_key(self.display.root_window(), keycode);
                    }
                    self.send_input_event(event)
                }
                self.display.sync();
                self.grabber.pop_state();
            }
            _ => {}
        }
    }

    fn grab_keys_for_window(&mut self, window: WindowRef) {
        info!("grab_keys_for_window {:?}", window);
        let mut to_grab = Vec::new();
        self.config.visit_key_mappings(&mut |k| {
            let code = match &k.input {
                KeySpec::Code(c) => Some(Keycode::try_from(*c as u8).expect("invalid keycode")),
                KeySpec::Sym(s) => {
                    let keysym = s.parse().expect("invalid keysym");
                    self.keyboard_mapping.keysym_to_keycode(keysym)
                }
            };
            if let Some(keycode) = code {
                to_grab.push((window, keycode, k.mods.mod_sets()));
            }
            ControlFlow::Continue
        });
        for (window, keycode, states) in to_grab {
            debug!(
                "grabbing key {:?} for {:?} with {} states",
                keycode,
                window,
                states.len()
            );
            for state in states {
                self.grabber.grab_key(window, keycode, state)
            }
            self.display.sync();
        }
    }

    fn handle_xevent(&mut self, _event: Event) {
        // match event {
        //     Event::CreateNotify { window } => self.grab_keys_for_window(window),
        // }
    }

    fn run() {
        let display = Display::new();

        let config = json5::from_str(include_str!("config.json5")).unwrap();
        info!("config: {:?}", config);
        let keyboard_mapping = display.get_keyboard_mapping();
        let modifier_mapping = display.get_modifier_mapping();

        let mut state = AppState {
            display,
            keys_down: Default::default(),
            config,
            keyboard_mapping,
            modifier_mapping,
            modifiers: Default::default(),
            ignore_queue: Default::default(),
            grabber: KeyGrabber::new(display),
        };

        state.grab_keys_for_window(state.display.root_window());

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
