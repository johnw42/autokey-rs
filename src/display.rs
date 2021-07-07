#![allow(dead_code)]

use enumset::EnumSet;
use libc::{c_int, c_uint, c_ulong, FD_ISSET, FD_SET, FD_ZERO};
use log::{info, trace};
use std::cmp::max;
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::mem::{size_of_val, MaybeUninit};
use std::ptr::{null, null_mut};
use x11::xlib::{
    AnyModifier, ButtonRelease, CreateNotify, Display as RawDisplay, GrabModeAsync, NoSymbol,
    StructureNotifyMask, SubstructureNotifyMask, Window as WindowId, XConnectionNumber,
    XDefaultRootWindow, XDisplayKeycodes, XEvent, XFree, XFreeModifiermap, XGetKeyboardMapping,
    XGetModifierMapping, XGrabKey, XNextEvent, XQueryTree, XSelectInput, XSync, XUngrabKey,
};
use x11::xtest::XTestFakeButtonEvent;
use x11::{
    xlib::{ButtonPress, KeyPress, KeyRelease, XOpenDisplay},
    xrecord::*,
    xtest::XTestFakeKeyEvent,
};

use crate::key::{Keycode, Keysym, Modifier};

#[derive(Clone, Copy)]
pub struct Display {
    // TODO: remove pub
    pub ptr: *mut RawDisplay,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct WindowRef {
    id: WindowId,
}

impl<'d> std::fmt::Debug for WindowRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl WindowRef {
    pub fn new(id: WindowId) -> Self {
        Self { id }
    }
}

pub enum Event {
    CreateNotify { window: WindowRef },
}

pub struct UnknownEventType(c_int);

impl Event {
    fn new(display: Display, event: XEvent) -> Result<Self, UnknownEventType> {
        // See https://docs.rs/x11/2.18.2/x11/xlib/union.XEvent.html
        unsafe {
            #[allow(non_upper_case_globals)]
            match event.any.type_ as c_int {
                CreateNotify => {
                    let event = event.create_window;
                    assert_eq!(event.display, display.ptr);
                    Ok(Event::CreateNotify {
                        window: WindowRef::new(event.window),
                    })
                }
                t => Err(UnknownEventType(t)),
            }
        }
    }
}

pub struct RecordingDisplay<'h> {
    ptr: *mut RawDisplay,
    handler: Box<Box<RecordingHandler<'h>>>,
}

pub type RecordingHandler<'h> = dyn FnMut(RecordedEvent) + 'h;

#[derive(Default)]
pub struct KeyboardMapping {
    pub keysym_to_keycode: HashMap<Keysym, Keycode>,
    pub keycode_to_keysyms: HashMap<Keycode, Vec<Keysym>>,
}

#[derive(Default)]
pub struct ModifierMapping {
    pub keycode_to_modifiers: HashMap<Keycode, EnumSet<Modifier>>,
}

// https://www.x.org/releases/X11R7.7/doc/xproto/x11protocol.html#Encoding::Events
#[derive(Debug)]
#[repr(C)]
struct RecordedEventData {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpOrDown {
    Up,
    Down,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Button {
    Key(Keycode),
    MouseButton(u8),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct InputEvent {
    pub direction: UpOrDown,
    pub button: Button,
}

#[derive(Debug)]
pub struct RecordedEvent {
    pub state: EnumSet<Modifier>,
    pub input: InputEvent,
}

struct UknownRecordedEvent;

impl TryFrom<&RecordedEventData> for RecordedEvent {
    type Error = UknownRecordedEvent;

    #[allow(non_upper_case_globals)]
    fn try_from(data: &RecordedEventData) -> Result<Self, Self::Error> {
        let state = EnumSet::<Modifier>::from_u16_truncated(data.state);
        let code = data.code as c_int;
        let direction = match code {
            KeyPress | ButtonPress => UpOrDown::Down,
            KeyRelease | ButtonRelease => UpOrDown::Up,
            _ => {
                debug_assert!(code < MIN_RECORDED_EVENT || code > MAX_RECORDED_EVENT);
                return Err(UknownRecordedEvent);
            }
        };
        let button = match code {
            KeyPress | KeyRelease => Keycode::try_from(data.detail).ok().map(Button::Key),
            ButtonPress | ButtonRelease => Some(Button::MouseButton(data.detail)),
            _ => unreachable!(),
        };
        button.map_or_else(
            || Err(UknownRecordedEvent),
            |button| {
                Ok(RecordedEvent {
                    state,
                    input: InputEvent { button, direction },
                })
            },
        )
    }
}

impl Display {
    pub fn new() -> Self {
        Display {
            ptr: unsafe { XOpenDisplay(null()) },
        }
    }

    pub fn sync(&self) {
        let discard = 0;
        unsafe {
            XSync(self.ptr, discard);
        }
    }

    pub fn get_keyboard_mapping(&self) -> KeyboardMapping {
        let mut mapping: KeyboardMapping = Default::default();
        unsafe {
            let mut min_keycode = 0;
            let mut max_keycode = 0;
            XDisplayKeycodes(self.ptr, &mut min_keycode, &mut max_keycode);
            let mut keysyms_per_keycode = 0;
            let keysyms = XGetKeyboardMapping(
                self.ptr,
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

    pub fn get_modifier_mapping(&self) -> ModifierMapping {
        let mut mod_mapping: ModifierMapping = Default::default();
        unsafe {
            let mapping = &mut *XGetModifierMapping(self.ptr);
            let mut ptr = mapping.modifiermap;
            for modifier in EnumSet::<Modifier>::all().iter() {
                for _ in 0..mapping.max_keypermod {
                    if let Ok(code) = Keycode::try_from(*ptr) {
                        mod_mapping
                            .keycode_to_modifiers
                            .entry(code)
                            .or_default()
                            .insert(modifier);
                    }
                    ptr = ptr.add(1);
                }
            }
            XFreeModifiermap(mapping);
        }
        mod_mapping
    }

    pub fn send_input_event(&self, event: InputEvent) -> Result<(), ()> {
        let is_press = match event.direction {
            UpOrDown::Up => 0,
            UpOrDown::Down => 1,
        };
        let delay = 0;
        let succeded = unsafe {
            // https://www.x.org/releases/X11R7.7/doc/libXtst/xtestlib.html
            match event.button {
                Button::Key(keycode) => {
                    //info!("XTestFakeKeyEvent {}", is_press);
                    XTestFakeKeyEvent(self.ptr, keycode.value() as c_uint, is_press, delay)
                }
                Button::MouseButton(button) => {
                    XTestFakeButtonEvent(self.ptr, button as c_uint, is_press, delay)
                }
            }
        };
        if succeded == 0 {
            Err(())
        } else {
            Ok(())
        }
    }

    pub fn visit_window_tree<F>(&self, window: WindowRef, f: &mut F)
    where
        F: FnMut(WindowRef),
    {
        unsafe {
            let mut root = 0;
            let mut parent = 0;
            let mut children = null_mut();
            let mut num_children = 0;
            if XQueryTree(
                self.ptr,
                window.id,
                &mut root,
                &mut parent,
                &mut children,
                &mut num_children,
            ) != 0
            {
                for i in 0..(num_children as usize) {
                    let child = *children.add(i);
                    self.visit_window_tree(WindowRef::new(child), f);
                }
                XFree(children as *mut _);
            }
        }
        trace!("visiting window {:?}", window);
        f(window);
    }

    pub fn grab_key(
        &self,
        window: WindowRef,
        keycode: Keycode,
        modifiers: Option<EnumSet<Modifier>>,
    ) {
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
            "grabbing key {} at {:?} with mods 0x{:x}",
            keycode, window, modifiers
        );
        unsafe {
            XGrabKey(
                self.ptr,
                keycode,
                modifiers,
                window.id,
                owner_events,
                pointer_mode,
                keyboard_mode,
            );
        }
        self.sync(); // TODO: remove
    }

    pub fn ungrab_key(
        &self,
        window: WindowRef,
        keycode: Keycode,
        modifiers: Option<EnumSet<Modifier>>,
    ) {
        assert!(
            modifiers.is_some(),
            "setting modifiers = None causes weird access errors"
        );
        let keycode = keycode.value().into();
        let modifiers = modifiers.map_or(AnyModifier, |s| s.as_u8().into());
        info!(
            "ungrabbing key {} at {:?} with mods 0x{:x}",
            keycode, window, modifiers
        );
        unsafe {
            XUngrabKey(self.ptr, keycode, modifiers, window.id);
        }
        self.sync(); // TODO: remove
    }

    pub fn root_window(&self) -> WindowRef {
        unsafe { WindowRef::new(XDefaultRootWindow(self.ptr)) }
    }

    pub fn event_loop<H>(&self, record_display: &RecordingDisplay, mut handler: H)
    where
        H: FnMut(Event),
    {
        unsafe {
            let root_window = XDefaultRootWindow(self.ptr);
            XSelectInput(
                self.ptr,
                root_window,
                StructureNotifyMask | SubstructureNotifyMask,
            );

            let main_fd = XConnectionNumber(self.ptr);
            let record_fd = XConnectionNumber(record_display.ptr);
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
                    XRecordProcessReplies(record_display.ptr);
                }
                if FD_ISSET(main_fd, &mut readfds) {
                    let mut event = MaybeUninit::uninit();
                    XNextEvent(self.ptr, event.as_mut_ptr());
                    if let Ok(event) = Event::new(*self, event.assume_init()) {
                        handler(event);
                    }
                }
            }
        }
    }
}

const MIN_RECORDED_EVENT: c_int = KeyPress;
const MAX_RECORDED_EVENT: c_int = ButtonRelease;

impl<'h> RecordingDisplay<'h> {
    pub fn new<H>(handler: H) -> Self
    where
        H: FnMut(RecordedEvent) + 'h,
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
                first: MIN_RECORDED_EVENT as u8,
                last: MAX_RECORDED_EVENT as u8,
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
            ptr: record_display,
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
    let event = &*(data.data as *const RecordedEventData);
    debug_assert_eq!((data.data_len * 4) as usize, size_of_val(event));
    match event.try_into() {
        Ok(event) => (*handler)(event),
        Err(_) => trace!("ignoring input event"),
    }
    XRecordFreeData(data as *mut _);
}
