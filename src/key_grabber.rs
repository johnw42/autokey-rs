use std::collections::{HashMap, HashSet};

use enumset::EnumSet;

use crate::{
    display::{Display, WindowRef},
    key::{Keycode, Modifier},
};

pub struct KeyGrabber {
    display: &Display,
    undo_stack: Vec<Grab>,
    active_grabs: HashMap<(WindowRef, Keycode), HashSet<EnumSet<Modifier>>>,
}

#[derive(Clone)]
struct Grab {
    window: WindowRef,
    keycode: Keycode,
    states: HashSet<EnumSet<Modifier>>,
}

impl KeyGrabber {
    pub fn new(display: &Display) -> Self {
        Self {
            display,
            undo_stack: Default::default(),
            active_grabs: Default::default(),
        }
    }

    pub fn push_state(
        &mut self,
        is_grabbed: bool,
        window: WindowRef,
        keycode: Keycode,
        states: HashSet<EnumSet<Modifier>>,
    ) {
        let grab = Grab {
            window,
            keycode,
            states,
        };
        self.undo_stack.push(grab.clone());
        self.set_state(grab, false);
    }

    pub fn pop_state(&mut self) {
        let grab = self.undo_stack.pop().expect("empty stack");
        self.set_state(grab, true);
    }

    fn set_state(
        &mut self,
        Grab {
            window,
            keycode,
            states,
        }: Grab,
        undo: bool,
    ) {
        let active_states = self.active_grabs.entry((window, keycode)).or_default();
        for &state in &states {
            if undo {
                if active_states.remove(&state) {
                    self.display.ungrab_key(window, keycode, Some(state))
                }
            } else {
                if active_states.insert(state) {
                    self.display.grab_key(window, keycode, Some(state))
                }
            }
        }
    }
}
