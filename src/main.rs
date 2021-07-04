#![allow(non_upper_case_globals)]
// mod x_interface;
// mod xcb_ext;

// use x_interface::*;

use libc::{c_int, c_uint, FD_ISSET, FD_SET, FD_ZERO};
use std::cmp::max;
use std::collections::BTreeSet;
use std::convert::TryFrom;
use std::mem::{size_of_val, MaybeUninit};
use std::ptr::{null, null_mut};
use x11::xlib::{
    CreateNotify, Display, GrabModeAsync, StructureNotifyMask, SubstructureNotifyMask, Window,
    XConnectionNumber, XDefaultRootWindow, XFree, XFreeModifiermap, XGetModifierMapping, XGrabKey,
    XKeycodeToKeysym, XNextEvent, XQueryTree, XSelectInput,
};
use x11::xtest::XTestFakeKeyEvent;
use x11::{
    xlib::{ButtonPress, KeyPress, KeyRelease, XOpenDisplay},
    xrecord::*,
};

mod key;
use key::*;

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

struct AppState {
    main_display: *mut Display,
    _record_display: *mut Display,
    keys_down: BTreeSet<Keycode>,
}

impl AppState {
    fn keycode_to_keysym(&self, keycode: Keycode) -> Option<Keysym> {
        unsafe {
            match XKeycodeToKeysym(self.main_display, keycode.value(), 0) {
                0 => None,
                n => Some(Keysym::from(n)),
            }
        }
    }

    fn keycode_to_string(&self, keycode: Keycode) -> String {
        self.keycode_to_keysym(keycode)
            .and_then(|k| k.to_string())
            .map(|s| format!("<{}>", s))
            .unwrap_or_else(|| format!("<keycode_{}>", keycode.value()))
    }

    fn write_key(&self, label: &str, keycode: Keycode, state: u16) {
        println!(
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

    fn handle_xrecord_event(&mut self, event: &RawEvent) {
        match event.code as c_int {
            KeyPress => {
                if let Ok(code) = Keycode::try_from(event.detail) {
                    self.keys_down.insert(code);
                    self.write_key("KeyPress", code, event.state);
                }
            }
            KeyRelease => {
                if let Ok(code) = Keycode::try_from(event.detail) {
                    self.keys_down.remove(&code);
                    self.write_key("KeyRelease", code, event.state);
                    if code == 52 && event.state == 0xc {
                        let press = 1;
                        let release = 0;
                        unsafe {
                            for &key in self.keys_down.iter() {
                                XTestFakeKeyEvent(
                                    self.main_display,
                                    key.value() as c_uint,
                                    release,
                                    0,
                                );
                            }
                            XTestFakeKeyEvent(self.main_display, 52, press, 0);
                            XTestFakeKeyEvent(self.main_display, 52, release, 0);
                            for &key in self.keys_down.iter() {
                                XTestFakeKeyEvent(
                                    self.main_display,
                                    key.value() as c_uint,
                                    press,
                                    0,
                                );
                            }
                        }
                    }
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
        f(self, window);
        unsafe {
            let mut root = 0;
            let mut parent = 0;
            let mut children = null_mut();
            let mut num_children = 0;
            if XQueryTree(
                self.main_display,
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
    }

    fn grab_keys(&mut self, window: Window) {
        self.visit_window_tree(window, &mut |self_, child| unsafe {
            XGrabKey(
                self_.main_display,
                52,
                0xc,
                child,
                0,
                GrabModeAsync,
                GrabModeAsync,
            );
        });
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
    println!("Hello, world!");

    let main_display = unsafe { XOpenDisplay(null()) };
    let record_display = unsafe { XOpenDisplay(null()) };
    unsafe {
        let mapping = &mut *XGetModifierMapping(main_display);
        let mut ptr = mapping.modifiermap;
        for mod_id in Modifier::values() {
            for _ in 0..mapping.max_keypermod {
                if let Ok(code) = Keycode::try_from(*ptr) {
                    println!("mod: {:?}, code: {}", mod_id, code.value());
                }
                ptr = ptr.add(1);
            }
        }
        XFreeModifiermap(mapping);
    }

    let mut app_state = AppState {
        main_display,
        _record_display: record_display,
        keys_down: Default::default(),
    };

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
            record_display,
            0,
            &mut clients[0],
            clients.len() as i32,
            &mut ranges[0],
            ranges.len() as i32,
        );
        assert_ne!(
            0,
            XRecordEnableContextAsync(
                record_display,
                context,
                Some(xrecord_callback),
                &mut app_state as *mut _ as *mut i8,
            )
        );

        let main_fd = XConnectionNumber(main_display);
        let record_fd = XConnectionNumber(record_display);

        let root_window = XDefaultRootWindow(main_display);
        app_state.grab_keys(root_window);
        XSelectInput(
            main_display,
            root_window,
            StructureNotifyMask | SubstructureNotifyMask,
        );
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
                XRecordProcessReplies(record_display);
            }
            if FD_ISSET(main_fd, &mut readfds) {
                let mut event = MaybeUninit::uninit();
                XNextEvent(main_display, event.as_mut_ptr());
                let event = event.assume_init();
                match event.any.type_ as c_int {
                    CreateNotify => {
                        let event = event.create_window;
                        app_state.grab_keys(event.window);
                    }
                    _ => {}
                }
            }
        }
    }
}
