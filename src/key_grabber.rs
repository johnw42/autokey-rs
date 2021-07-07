use std::collections::{HashMap, HashSet};

use enumset::EnumSet;

use crate::{
    display::{Display, WindowRef},
    key::{Keycode, Modifier},
};

pub struct KeyGrabber {
    display: Display,
    current_grabs: Vec<Grab>,
    undo_stack: Vec<Vec<Grab>>,
    active_grabs: HashMap<(WindowRef, Keycode), HashSet<EnumSet<Modifier>>>,
}

#[derive(Clone)]
struct Grab {
    window: WindowRef,
    keycode: Keycode,
    state: EnumSet<Modifier>,
    is_grabbed: bool,
}

impl KeyGrabber {
    pub fn new(display: Display) -> Self {
        Self {
            display,
            undo_stack: Default::default(),
            active_grabs: Default::default(),
            current_grabs: Default::default(),
        }
    }

    // pub fn push_state(
    //     &mut self,
    //     is_grabbed: bool,
    //     window: WindowRef,
    //     keycode: Keycode,
    //     states: HashSet<EnumSet<Modifier>>,
    // ) {
    //     let grab = Grab {
    //         window,
    //         keycode,
    //         states,
    //     };
    //     self.undo_stack.push(grab.clone());
    //     self.set_state(grab, false);
    // }

    pub fn grab_key(&mut self, window: WindowRef, keycode: Keycode, state: EnumSet<Modifier>) {
        let grab = Grab {
            window,
            keycode,
            state,
            is_grabbed: true,
        };
        if self.apply_grab(grab.clone(), false) {
            self.current_grabs.push(grab);
        }
    }

    pub fn ungrab_key(&mut self, window: WindowRef, keycode: Keycode) {
        let grabs = self.active_grabs.get(&(window, keycode)).cloned();
        if let Some(grabs) = grabs {
            for state in grabs {
                let grab = Grab {
                    window,
                    keycode,
                    state,
                    is_grabbed: false,
                };
                if self.apply_grab(grab.clone(), false) {
                    self.current_grabs.push(grab);
                }
            }
        }
    }

    pub fn push_state(&mut self) {
        self.undo_stack
            .push(std::mem::take(&mut self.current_grabs));
    }

    pub fn pop_state(&mut self) {
        let grabs = self.undo_stack.pop().expect("empty stack");
        for grab in grabs.into_iter().rev() {
            self.apply_grab(grab, true);
        }
    }

    fn apply_grab(
        &mut self,
        Grab {
            window,
            keycode,
            state,
            is_grabbed,
        }: Grab,
        undo: bool,
    ) -> bool {
        let active_states = self.active_grabs.entry((window, keycode)).or_default();
        if undo == is_grabbed {
            if active_states.remove(&state) {
                self.display.ungrab_key(window, keycode, Some(state));
                return true;
            }
        } else {
            if active_states.insert(state) {
                self.display.grab_key(window, keycode, Some(state));
                return true;
            }
        }
        false
    }
}
