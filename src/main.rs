mod config;
mod display;
mod key;
mod key_grabber;

use crate::{
    config::{Config, ValidConfig},
    key::Keycode,
    key_grabber::KeyGrabber,
};
use display::{
    Button, Display, Event, InputEvent, RecordedEvent, RecordingDisplay, UpOrDown, WindowRef,
};
use enumset::EnumSet;
use key::{KeyboardMapping, Modifier, ModifierMapping};
use lazy_static::lazy_static;
use log::{debug, error, info, trace, LevelFilter};
use nix::{
    libc::c_int,
    sys::{
        signal::{signal, SigHandler, Signal},
        wait::waitpid,
    },
    unistd::{fork, getpid, ForkResult, Pid},
};
use std::{
    cell::RefCell,
    collections::{BTreeSet, VecDeque},
    convert::TryFrom,
    env, panic,
    sync::Mutex,
};
use syslog::{BasicLogger, Facility, Formatter3164};

struct AppState {
    display: Display,
    keys_down: BTreeSet<Keycode>,
    _config: Config,
    valid_config: ValidConfig,
    _keyboard_mapping: KeyboardMapping,
    modifier_mapping: ModifierMapping,
    modifiers: EnumSet<Modifier>,
    ignore_queue: VecDeque<InputEvent>,
    grabber: KeyGrabber,
}

impl AppState {
    fn _keycode_to_string(&self, keycode: Keycode) -> String {
        self._keyboard_mapping
            ._keycode_to_keysym(keycode)
            .and_then(|k| k.to_string())
            .map(|s| format!("<{}>", s))
            .unwrap_or_else(|| format!("<keycode_{}>", keycode.value()))
    }

    fn _log_key(&self, label: &str, keycode: Keycode, state: EnumSet<Modifier>) {
        debug!(
            "{}: code={}, sym={} ({:?}), state={:?}, down=[{}]",
            label,
            keycode.value(),
            self._keyboard_mapping
                ._keycode_to_keysym(keycode)
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
        debug!("sending event: {:?}", event);
        match self.display.send_input_event(event.clone()) {
            Ok(_) => self.ignore_queue.push_back(event),
            Err(_) => error!("error sending input event: {:?}", event),
        }
    }

    fn handle_recorded_event(&mut self, event: RecordedEvent) {
        use Button::*;
        use UpOrDown::*;

        debug!("handling input event: {:?}", event);

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
            .flat_map(|&keycode| self.modifier_mapping.keycode_to_modifier(keycode))
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
                let mut key_mapping = None;
                let mut to_send = Vec::new();
                for k in &self.valid_config.key_mappings {
                    if let Button::Key(keycode) = *button {
                        if keycode == k.input && k.mods.matches(self.modifiers) {
                            key_mapping = Some(k);
                            for chord in &k.output {
                                for keycode in chord.iter().copied() {
                                    to_send.push(InputEvent {
                                        button: Button::Key(keycode),
                                        direction: UpOrDown::Down,
                                    });
                                }
                                for keycode in chord.iter().rev().copied() {
                                    to_send.push(InputEvent {
                                        button: Button::Key(keycode),
                                        direction: UpOrDown::Up,
                                    });
                                }
                            }
                            break;
                        }
                    }
                }
                if let Some(key_mapping) = key_mapping {
                    self.display.flush();
                    self.grabber.push_state();
                    debug_assert!(self.modifiers.is_superset(key_mapping.mods.required_set()));
                    debug_assert!(self.modifiers.is_disjoint(key_mapping.mods.forbidden_set()));
                    let modifiers = self.modifiers & key_mapping.mods.allowed_set();
                    self.with_modifiers(modifiers, |inner_self| {
                        for event in to_send.into_iter() {
                            if let Button::Key(keycode) = event.button {
                                inner_self
                                    .grabber
                                    .ungrab_key(inner_self.display.root_window(), keycode);
                            }
                            inner_self.send_input_event(event)
                        }
                    });
                    self.display.flush();
                    self.grabber.pop_state();
                }
            }
            _ => {}
        }
    }

    fn with_modifiers<F>(&mut self, modifiers: EnumSet<Modifier>, f: F)
    where
        F: FnOnce(&mut Self),
    {
        let to_release: Vec<Keycode> = self
            .keys_down
            .iter()
            .copied()
            .filter(|&keycode| {
                self.modifier_mapping
                    .keycode_to_modifier(keycode)
                    .map_or(false, |m| !modifiers.contains(m))
            })
            .collect();
        let to_press: Vec<Keycode> = (modifiers - self.modifiers)
            .iter()
            .filter_map(|m| {
                self.modifier_mapping
                    .modifier_to_keycodes(m)
                    .first()
                    .copied()
            })
            .collect();

        debug_assert!(to_release.iter().all(|k| !to_press.contains(k)));
        debug_assert!(to_press.iter().all(|k| !to_release.contains(k)));

        self.grabber.push_state();
        for &keycode in &to_release {
            self.grabber.ungrab_key(self.display.root_window(), keycode);
            self.send_input_event(InputEvent {
                button: Button::Key(keycode),
                direction: UpOrDown::Up,
            });
        }
        for &keycode in &to_press {
            self.grabber.ungrab_key(self.display.root_window(), keycode);
            self.send_input_event(InputEvent {
                button: Button::Key(keycode),
                direction: UpOrDown::Down,
            });
        }
        f(self);
        for &keycode in &to_press {
            self.send_input_event(InputEvent {
                button: Button::Key(keycode),
                direction: UpOrDown::Up,
            });
        }
        for &keycode in &to_release {
            self.send_input_event(InputEvent {
                button: Button::Key(keycode),
                direction: UpOrDown::Down,
            });
        }
        self.grabber.pop_state();
    }

    fn grab_keys_for_window(&mut self, window: WindowRef) {
        debug!("grab_keys_for_window {:?}", window);

        for k in &self.valid_config.key_mappings {
            let states = k.mods.mod_sets();
            trace!(
                "grabbing key {:?} for {:?} with {} states",
                k.input,
                window,
                states.len()
            );
            for state in states {
                self.grabber.grab_key(window, k.input, state)
            }
        }
    }

    fn handle_xevent(&mut self, _event: Event) {
        // match event {
        //     Event::CreateNotify { window } => self.grab_keys_for_window(window),
        // }
    }

    fn run() {
        let display = Display::new();

        let config: Config = json5::from_str(include_str!("config.json5")).unwrap();
        debug!("config: {:?}", config);
        let keyboard_mapping = display.get_keyboard_mapping();
        let modifier_mapping = display.get_modifier_mapping();

        let mut state = AppState {
            display,
            keys_down: Default::default(),
            valid_config: config.validate(&keyboard_mapping),
            _config: config,
            _keyboard_mapping: keyboard_mapping,
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

fn run_as_daemon() {
    let formatter = Formatter3164 {
        facility: Facility::LOG_USER,
        hostname: None,
        process: "autokey-rs".into(),
        pid: getpid().into(),
    };
    let logger = syslog::unix(formatter).unwrap();
    log::set_boxed_logger(Box::new(BasicLogger::new(logger))).unwrap();
    log::set_max_level(LevelFilter::Info);

    // Make sure panics are logged.
    panic::set_hook(Box::new(|info| {
        let message: &str = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| *s)
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("(no message)");
        if let Some(loc) = info.location() {
            error!("panic at {}:{}: {}", loc.file(), loc.line(), message);
        } else {
            error!("panic: {}", message);
        }
    }));

    enum State {
        Starting,
        AwaitingChild(Pid),
        ShuttingDown,
    }

    lazy_static! {
        static ref STATE: Mutex<State> = Mutex::new(State::Starting);
    }

    extern "C" fn on_signal(signal: c_int) {
        let signal = Signal::try_from(signal).expect("invalid signal");
        let mut state = STATE.lock().expect("failed to acquire lock");
        if let State::AwaitingChild(child) = *state {
            let _ = nix::sys::signal::kill(child, signal);
        }
        if signal == Signal::SIGTERM {
            *state = State::ShuttingDown;
        }
    }

    loop {
        let mut state = STATE.lock().unwrap();
        if let State::ShuttingDown = *state {
            break;
        }
        match unsafe { fork() }.unwrap() {
            ForkResult::Parent { child } => {
                *state = State::AwaitingChild(child);
                drop(state);
                unsafe {
                    let msg = "failed to install signal handler";
                    signal(Signal::SIGQUIT, SigHandler::Handler(on_signal)).expect(msg);
                    signal(Signal::SIGTERM, SigHandler::Handler(on_signal)).expect(msg);
                }
                info!("monitoring child {}", child);
                while let Err(err) = waitpid(child, None) {
                    error!("error waiting child: {:?}", err);
                }
            }
            ForkResult::Child => {
                drop(state);
                unsafe {
                    let msg = "failed to clear signal handler";
                    signal(Signal::SIGQUIT, SigHandler::SigDfl).expect(msg);
                    signal(Signal::SIGTERM, SigHandler::SigDfl).expect(msg);
                }
                info!("spawned child: {}", getpid());
                AppState::run();
                break;
            }
        }
    }
}

fn main() {
    if env::args().any(|arg| arg == "--daemon") {
        run_as_daemon();
    } else {
        env_logger::init();
        AppState::run();
    }
}
