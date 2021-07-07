use std::collections::{HashMap, HashSet};

use enumset::EnumSet;
use log::info;

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

#[derive(Clone, Debug)]
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
        self.apply_grab(grab.clone(), false);
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
                self.apply_grab(grab.clone(), false);
            }
        }
    }

    pub fn push_state(&mut self) {
        info!("pushing state with {} items", self.current_grabs.len());
        self.undo_stack
            .push(std::mem::take(&mut self.current_grabs));
    }

    pub fn pop_state(&mut self) {
        let grabs = self.undo_stack.pop().expect("empty stack");
        info!("popping state with {} items", grabs.len());
        for grab in grabs.iter().rev() {
            self.apply_grab(grab.clone(), true);
        }
        self.current_grabs = grabs;
        info!("popped state with {} items", self.current_grabs.len());
    }

    fn apply_grab(&mut self, grab: Grab, undo: bool) {
        let Grab {
            window,
            keycode,
            state,
            is_grabbed,
        } = grab.clone();
        let active_states = self.active_grabs.entry((window, keycode)).or_default();
        if state.is_empty() {
            info!("applying grab: {:?}", grab);
            info!("active grab? {}", active_states.contains(&state));
        }
        // info!("num active states: {}", active_states.len());7
        let did_update = if !is_grabbed {
            if active_states.remove(&state) {
                if state.is_empty() {
                    info!("ungrabbing {:?}", keycode);
                }
                self.display.ungrab_key(window, keycode, Some(state));
                true
            } else {
                false
            }
        } else {
            if active_states.insert(state) {
                if state.is_empty() {
                    info!("grabbing {:?}", keycode);
                }
                self.display.grab_key(window, keycode, Some(state));
                true
            } else {
                false
            }
        };

        if did_update && !undo {
            self.current_grabs.push(grab);
        }

        if state.is_empty() {
            info!(
                "did_update? {}; undo? {}; current_grabs.len() == {}",
                did_update,
                undo,
                self.current_grabs.len()
            );
        }
    }
}
