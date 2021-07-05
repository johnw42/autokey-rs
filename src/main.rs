#![allow(non_upper_case_global)]
// mod x_interface;
// mod xcb_ext;

// use x_interface::*;

use config::Config;
use enumset::EnumSet;
use libc::{c_int, c_uint, c_ulong, FD_ISSET, FD_SET, FD_ZERO};
use log::{info, trace};
use std::cmp::max;
use std::collections::{BTreeSet, HashMap};
use std::convert::TryFrom;
use std::mem::{size_of_val, MaybeUninit};
use std::ptr::{null, null_mut};
use std::thread::sleep;
use std::time::Duration;
use x11::xlib::{
    AnyModifier, CreateNotify, Display, GrabModeAsync, NoSymbol, StructureNotifyMask,
    SubstructureNotifyMask, Window, XConnectionNumber, XDefaultRootWindow, XDisplayKeycodes, XFree,
    XFreeModifiermap, XGetKeyboardMapping, XGetModifierMapping, XGrabKey, XNextEvent, XQueryTree,
    XSelectInput,
};
use x11::xtest::XTestFakeKeyEvent;
use x11::{
    xlib::{ButtonPress, KeyPress, KeyRelease, XOpenDisplay},
    xrecord::*,
};

mod config;
mod key;

use key::*;

use crate::config::{ConfigItem, KeySpec};

// https://www.x.org/releases/X11R7.7/doc/xproto/x11protocol.html#Encoding::Events
#[derive(Debug)]
#[repr(C)]
struct RawEvent {
    code: u8,
    detail: u8,
    seq_num: u16,
    time: u32,
    root: u32,
    event: u32,
    child: u32,
    root_x: u16,
    root_y: u16,
    event_x: u16,
    event_y: u16,
    state: u16,
    same_screen: u8,
    unused: u8,
}

struct Connection {
    display: *mut Display,
}

struct AppState {
    display: *mut Display,
    record_display: *mut Display,
    keys_down: BTreeSet<Keycode>,
    config: Config,
    keysym_to_keycode: HashMap<Keysym, Keycode>,
    keycode_to_keysyms: HashMap<Keycode, Vec<Keysym>>,
    modifiers: EnumSet<Modifier>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum KeyEvent {
    Press,
    Release,
}

impl AppState {
    fn keysym_to_keycode(&self, keysym: Keysym) -> Option<Keycode> {
        self.keysym_to_keycode.get(&keysym).copied()
    }

    fn get_keyboard_mapping(&mut self) {
        unsafe {
            let mut min_keycode = 0;
            let mut max_keycode = 0;
            XDisplayKeycodes(self.display, &mut min_keycode, &mut max_keycode);
            let mut keysyms_per_keycode = 0;
            let keysyms = XGetKeyboardMapping(
                self.display,
                min_keycode as u8,
                max_keycode - min_keycode + 1,
                &mut keysyms_per_keycode,
            );
            let mut ptr = keysyms;
            self.keysym_to_keycode.clear();
            self.keycode_to_keysyms.clear();
            for keycode in min_keycode..=max_keycode {
                let keycode = Keycode::try_from(keycode as u8).expect("invalid keycode");
                for _ in 0..keysyms_per_keycode {
                    let keysym = *ptr;
                    ptr = ptr.add(1);
                    if keysym != NoSymbol as c_ulong {
                        let keysym = Keysym::from(keysym);
                        self.keysym_to_keycode.insert(keysym, keycode);
                        self.keycode_to_keysyms
                            .entry(keycode)
                            .or_default()
                            .push(keysym);
                    }
                }
            }
            XFree(keysyms as *mut _);
        }
    }

    fn keycode_to_keysym(&self, keycode: Keycode) -> Option<Keysym> {
        self.keycode_to_keysyms.get(&keycode).map(|v| v[0])
    }

    fn keycode_to_string(&self, keycode: Keycode) -> String {
        self.keycode_to_keysym(keycode)
            .and_then(|k| k.to_string())
            .map(|s| format!("<{}>", s))
            .unwrap_or_else(|| format!("<keycode_{}>", keycode.value()))
    }

    fn log_key(&self, label: &str, keycode: Keycode, state: u16) {
        trace!(
            "{}: code={}, sym={} ({:?}), state={}, down=[{}]",
            label,
            keycode.value(),
            self.keycode_to_keysym(keycode)
                .map(|k| k.value())
                .unwrap_or(0),
            self.keycode_to_string(keycode),
            state,
            self.keys_down
                .iter()
                .map(|&k| self.keycode_to_string(k))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    fn send_key_event(&self, keycode: Keycode, key_event: KeyEvent) {
        unsafe {
            XTestFakeKeyEvent(
                self.display,
                keycode.value() as c_uint,
                match key_event {
                    KeyEvent::Press => 1,
                    KeyEvent::Release => 0,
                },
                0,
            );
        }
    }

    fn handle_xrecord_event(&mut self, event: &RawEvent) {
        match event.code as c_int {
            KeyPress => {
                if let Ok(code) = Keycode::try_from(event.detail) {
                    self.keys_down.insert(code);
                    self.log_key("KeyPress", code, event.state);
                }
            }
            KeyRelease => {
                if let Ok(code) = Keycode::try_from(event.detail) {
                    self.keys_down.remove(&code);
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

    fn visit_window_tree<F>(&mut self, window: Window, f: &mut F)
    where
        F: FnMut(&mut Self, Window),
    {
        info!("visiting {}", window);
        //sleep(Duration::from_millis(200));
        unsafe {
            let mut root = 0;
            let mut parent = 0;
            let mut children = null_mut();
            let mut num_children = 0;
            info!("querying tree of {}", window);
            //sleep(Duration::from_millis(200));
            if XQueryTree(
                self.display,
                window,
                &mut root,
                &mut parent,
                &mut children,
                &mut num_children,
            ) != 0
            {
                for i in 0..(num_children as usize) {
                    let child = *children.add(i);
                    self.visit_window_tree(child, f);
                }
                XFree(children as *mut _);
            }
        }
        f(self, window);
    }

    fn grab_key(&self, window: Window, keycode: Keycode, modifiers: Option<EnumSet<Modifier>>) {
        let keycode = keycode.value().into();
        let modifiers = modifiers.map_or(AnyModifier, |s| s.as_u8().into());
        let owner_events = 1;
        let pointer_mode = GrabModeAsync;
        let keyboard_mode = GrabModeAsync;
        //info!("grabbing key {} at window {}", keycode, window);
        //sleep(Duration::from_millis(200));
        unsafe {
            XGrabKey(
                self.display,
                keycode,
                modifiers,
                window,
                owner_events,
                pointer_mode,
                keyboard_mode,
            );
        }
    }

    fn grab_keys_for_window(&mut self, window: Window) {
        info!("starting traversal at window; {}", window);
        //sleep(Duration::from_millis(200));
        self.visit_window_tree(window, &mut |s, child| {
            info!("visiting window: {}", child);
            //sleep(Duration::from_millis(200));
            // let s = self;
            // let child = window;
            s.config.visit_key_mappings(&|k| match k.input {
                KeySpec::Code(c) => {
                    s.grab_key(
                        child,
                        Keycode::try_from(c as u8).expect("invalid keycode"),
                        None,
                    );
                }
                KeySpec::Sym(_) => {}
            })
        });
    }

    fn start_recording(&mut self) {
        assert!(self.record_display.is_null());

        self.record_display = unsafe { XOpenDisplay(null()) };

        let mut clients = [XRecordAllClients];
        let range = unsafe { &mut *XRecordAllocRange() };
        *range = XRecordRange {
            core_requests: XRecordRange8 { first: 0, last: 0 },
            core_replies: XRecordRange8 { first: 0, last: 0 },
            ext_requests: XRecordExtRange {
                ext_major: XRecordRange8 { first: 0, last: 0 },
                ext_minor: XRecordRange16 { first: 0, last: 0 },
            },
            ext_replies: XRecordExtRange {
                ext_major: XRecordRange8 { first: 0, last: 0 },
                ext_minor: XRecordRange16 { first: 0, last: 0 },
            },
            delivered_events: XRecordRange8 { first: 0, last: 0 },
            device_events: XRecordRange8 {
                first: KeyPress as u8,
                last: ButtonPress as u8,
            },
            errors: XRecordRange8 { first: 0, last: 0 },
            client_started: 0,
            client_died: 0,
        };
        let mut ranges = [range as *mut _];
        unsafe {
            let context = XRecordCreateContext(
                self.record_display,
                0,
                &mut clients[0],
                clients.len() as i32,
                &mut ranges[0],
                ranges.len() as i32,
            );
            assert_ne!(
                0,
                XRecordEnableContextAsync(
                    self.record_display,
                    context,
                    Some(xrecord_callback),
                    self as *mut _ as *mut i8,
                )
            );
        }
    }

    fn event_loop(&mut self) {
        unsafe {
            let root_window = XDefaultRootWindow(self.display);
            self.grab_keys_for_window(root_window);

            XSelectInput(
                self.display,
                root_window,
                StructureNotifyMask | SubstructureNotifyMask,
            );

            let main_fd = XConnectionNumber(self.display);
            let record_fd = XConnectionNumber(self.record_display);
            loop {
                let mut readfs = MaybeUninit::uninit();
                FD_ZERO(readfs.as_mut_ptr());
                let mut readfds = readfs.assume_init();
                FD_SET(main_fd, &mut readfds);
                FD_SET(record_fd, &mut readfds);
                libc::select(
                    max(dbg!(main_fd), dbg!(record_fd)) + 1,
                    &mut readfds,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                );
                if FD_ISSET(record_fd, &mut readfds) {
                    XRecordProcessReplies(self.record_display);
                }
                if FD_ISSET(main_fd, &mut readfds) {
                    let mut event = MaybeUninit::uninit();
                    XNextEvent(self.display, event.as_mut_ptr());
                    let event = event.assume_init();
                    match event.any.type_ as c_int {
                        CreateNotify => {
                            let event = event.create_window;
                            self.grab_keys_for_window(event.window);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

unsafe extern "C" fn xrecord_callback(app_state: *mut i8, data: *mut XRecordInterceptData) {
    let data = &mut *data;
    if data.category != XRecordFromServer || data.client_swapped != 0 || data.data_len == 0 {
        return;
    }
    let app_state = &mut *(app_state as *mut AppState);
    let event = &*(data.data as *const RawEvent);
    debug_assert_eq!((data.data_len * 4) as usize, size_of_val(event));
    app_state.handle_xrecord_event(event);
    XRecordFreeData(data as *mut _);
}

fn main() {
    env_logger::init();

    let main_display = unsafe { XOpenDisplay(null()) };
    unsafe {
        let mapping = &mut *XGetModifierMapping(main_display);
        let mut ptr = mapping.modifiermap;
        for modifier in EnumSet::<Modifier>::all().iter() {
            for _ in 0..mapping.max_keypermod {
                if let Ok(code) = Keycode::try_from(*ptr) {
                    println!("mod: {:?}, code: {}", modifier, code.value());
                }
                ptr = ptr.add(1);
            }
        }
        XFreeModifiermap(mapping);
    }

    let mut config = json5::from_str(include_str!("config.json5")).unwrap();

    let mut app_state = AppState {
        display: main_display,
        record_display: null_mut(),
        keys_down: Default::default(),
        config: Default::default(),
        keysym_to_keycode: Default::default(),
        keycode_to_keysyms: Default::default(),
        modifiers: Default::default(),
    };

    app_state.get_keyboard_mapping();

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
    info!("config: {:?}", config);

    app_state.config = config;
    app_state.start_recording();
    app_state.event_loop();
}
