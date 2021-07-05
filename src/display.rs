#![allow(dead_code)]

use enumset::EnumSet;
use libc::{c_uint, c_ulong, FD_ISSET, FD_SET, FD_ZERO};
use log::{info, trace};
use std::cmp::max;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::mem::{size_of_val, MaybeUninit};
use std::ptr::{null, null_mut};
use x11::xlib::{
    AnyModifier, Display as RawDisplay, GrabModeAsync, NoSymbol, StructureNotifyMask,
    SubstructureNotifyMask, Window, XConnectionNumber, XDefaultRootWindow, XDisplayKeycodes,
    XEvent, XFree, XFreeModifiermap, XGetKeyboardMapping, XGetModifierMapping, XGrabKey,
    XNextEvent, XQueryTree, XSelectInput,
};
use x11::xtest::XTestFakeKeyEvent;
use x11::{
    xlib::{ButtonPress, KeyPress, XOpenDisplay},
    xrecord::*,
};

use crate::key::{Keycode, Keysym, Modifier};

pub struct Display {
    display: *mut RawDisplay,
}

pub struct RecordingDisplay<'h> {
    record_display: *mut RawDisplay,
    handler: Box<Box<RecordingHandler<'h>>>,
}

pub type RecordingHandler<'h> = dyn FnMut(&RecordedEvent) + 'h;

#[derive(Default)]
pub struct KeyboardMapping {
    pub keysym_to_keycode: HashMap<Keysym, Keycode>,
    pub keycode_to_keysyms: HashMap<Keycode, Vec<Keysym>>,
}

// https://www.x.org/releases/X11R7.7/doc/xproto/x11protocol.html#Encoding::Events
#[derive(Debug)]
#[repr(C)]
pub struct RecordedEvent {
    pub code: u8,
    pub detail: u8,
    seq_num: u16,
    time: u32,
    root: u32,
    event: u32,
    child: u32,
    root_x: u16,
    root_y: u16,
    event_x: u16,
    event_y: u16,
    pub state: u16,
    same_screen: u8,
    unused: u8,
}

// struct AppState {
//     display: *mut RawDisplay,
//     record_display: *mut RawDisplay,
//     keys_down: BTreeSet<Keycode>,
//     config: Config,
//     keysym_to_keycode: HashMap<Keysym, Keycode>,
//     keycode_to_keysyms: HashMap<Keycode, Vec<Keysym>>,
//     modifiers: EnumSet<Modifier>,
// }

#[derive(Clone, Copy)]
pub enum KeyEvent {
    Press,
    Release,
}

impl Display {
    pub fn new() -> Self {
        Display {
            display: unsafe { XOpenDisplay(null()) },
        }
    }

    pub fn get_keyboard_mapping(&self) -> KeyboardMapping {
        let mut mapping: KeyboardMapping = Default::default();
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
            for keycode in min_keycode..=max_keycode {
                let keycode = Keycode::try_from(keycode as u8).expect("invalid keycode");
                for _ in 0..keysyms_per_keycode {
                    let keysym = *ptr;
                    ptr = ptr.add(1);
                    if keysym != NoSymbol as c_ulong {
                        let keysym = Keysym::from(keysym);
                        mapping.keysym_to_keycode.insert(keysym, keycode);
                        mapping
                            .keycode_to_keysyms
                            .entry(keycode)
                            .or_default()
                            .push(keysym);
                    }
                }
            }
            XFree(keysyms as *mut _);
        }
        mapping
    }

    pub fn get_modifier_mapping(&self) -> HashMap<Modifier, Vec<Keycode>> {
        let mut hash_map: HashMap<Modifier, Vec<Keycode>> = HashMap::new();
        unsafe {
            let mapping = &mut *XGetModifierMapping(self.display);
            let mut ptr = mapping.modifiermap;
            for modifier in EnumSet::<Modifier>::all().iter() {
                for _ in 0..mapping.max_keypermod {
                    if let Ok(code) = Keycode::try_from(*ptr) {
                        hash_map.entry(modifier).or_default().push(code);
                    }
                    ptr = ptr.add(1);
                }
            }
            XFreeModifiermap(mapping);
        }
        hash_map
    }

    pub fn send_key_event(&self, keycode: Keycode, key_event: KeyEvent) {
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

    pub fn visit_window_tree<F>(&self, window: Window, f: &mut F)
    where
        F: FnMut(Window),
    {
        trace!("visiting window {}", window);
        f(window);
        unsafe {
            let mut root = 0;
            let mut parent = 0;
            let mut children = null_mut();
            let mut num_children = 0;
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
    }

    pub fn grab_key(&self, window: Window, keycode: Keycode, modifiers: Option<EnumSet<Modifier>>) {
        assert!(
            modifiers.is_some(),
            "setting modifiers = None causes weird access errors"
        );
        let keycode = keycode.value().into();
        let modifiers = modifiers.map_or(AnyModifier, |s| s.as_u8().into());
        let owner_events = 0;
        let pointer_mode = GrabModeAsync;
        let keyboard_mode = GrabModeAsync;
        info!(
            "grabbing key {} at window {} with mods 0x{:x}",
            keycode, window, modifiers
        );
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

    pub fn event_loop<H>(&self, record_display: &RecordingDisplay, mut handler: H)
    where
        H: FnMut(XEvent),
    {
        unsafe {
            let root_window = XDefaultRootWindow(self.display);
            XSelectInput(
                self.display,
                root_window,
                StructureNotifyMask | SubstructureNotifyMask,
            );

            let main_fd = XConnectionNumber(self.display);
            let record_fd = XConnectionNumber(record_display.record_display);
            loop {
                let mut readfs = MaybeUninit::uninit();
                FD_ZERO(readfs.as_mut_ptr());
                let mut readfds = readfs.assume_init();
                FD_SET(main_fd, &mut readfds);
                FD_SET(record_fd, &mut readfds);
                libc::select(
                    max(main_fd, record_fd) + 1,
                    &mut readfds,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                );
                if FD_ISSET(record_fd, &mut readfds) {
                    XRecordProcessReplies(record_display.record_display);
                }
                if FD_ISSET(main_fd, &mut readfds) {
                    let mut event = MaybeUninit::uninit();
                    XNextEvent(self.display, event.as_mut_ptr());
                    handler(event.assume_init());
                }
            }
        }
    }
}

impl<'h> RecordingDisplay<'h> {
    pub fn new<H>(handler: H) -> Self
    where
        H: FnMut(&RecordedEvent) + 'h,
    {
        let record_display = unsafe { XOpenDisplay(null()) };
        let handler: Box<RecordingHandler<'h>> = Box::new(handler);
        let mut handler = Box::new(handler);

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
                    handler.as_mut() as *mut Box<RecordingHandler<'h>> as *mut i8,
                )
            );
        }

        RecordingDisplay {
            record_display,
            handler,
        }
    }
}

unsafe extern "C" fn xrecord_callback(handler: *mut i8, data: *mut XRecordInterceptData) {
    let data = &mut *data;
    if data.category != XRecordFromServer || data.client_swapped != 0 || data.data_len == 0 {
        return;
    }
    let handler = handler as *mut Box<RecordingHandler<'static>>;
    let event = &*(data.data as *const RecordedEvent);
    debug_assert_eq!((data.data_len * 4) as usize, size_of_val(event));
    (*handler)(event);
    XRecordFreeData(data as *mut _);
}
