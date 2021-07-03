// mod x_interface;
// mod xcb_ext;

// use x_interface::*;

use libc::{c_int, c_uchar, c_ulong};
use std::collections::BTreeSet;
use std::ffi::CStr;
use std::mem::size_of_val;
use std::ptr::null;
use x11::xlib::{Display, XKeycodeToKeysym, XKeysymToString};
use x11::{
    xlib::{ButtonPress, KeyPress, KeyRelease, XOpenDisplay},
    xrecord::*,
};

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

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Debug)]
struct Keycode(c_uchar);

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Debug)]
struct Keysym(c_ulong);

impl Keysym {
    fn to_c_str(&self) -> Option<&'static CStr> {
        unsafe {
            XKeysymToString(self.0)
                .as_ref()
                .map(|ptr| CStr::from_ptr(ptr))
        }
    }
}

struct AppState {
    main_display: *mut Display,
    _record_display: *mut Display,
    keys_down: BTreeSet<Keycode>,
}

impl AppState {
    fn keycode_to_keysym(&self, keycode: Keycode) -> Option<Keysym> {
        unsafe {
            match XKeycodeToKeysym(self.main_display, keycode.0, 0) {
                0 => None,
                n => Some(Keysym(n)),
            }
        }
    }

    fn keycode_to_string(&self, keycode: Keycode) -> String {
        self.keycode_to_keysym(keycode)
            .and_then(|k| k.to_c_str())
            .map(|s| format!("<{}>", s.to_string_lossy()))
            .unwrap_or_else(|| format!("<keycode_{}>", keycode.0))
    }

    fn write_key(&self, label: &str, keycode: Keycode, state: u16) {
        println!(
            "{}: code={}, sym={} ({:?}), state={}, down=[{}]",
            label,
            keycode.0,
            self.keycode_to_keysym(keycode).unwrap_or(Keysym(0)).0,
            self.keycode_to_string(keycode),
            state,
            self.keys_down
                .iter()
                .map(|&k| self.keycode_to_string(k))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    #[allow(non_upper_case_globals)]
    fn handle_xrecord_event(&mut self, event: &RawEvent) {
        match event.code as c_int {
            KeyPress => {
                let code = Keycode(event.detail);
                self.keys_down.insert(code);
                self.write_key("KeyPress", code, event.state);
            }
            KeyRelease => {
                let code = Keycode(event.detail);
                self.keys_down.remove(&code);
                self.write_key("KeyRelease", code, event.state);
            }
            ButtonPress => {
                println!("ButtonPress: {} {:x}", event.detail, event.state);
            }
            _ => {
                println!("event type: {:?}", event);
            }
        }
    }
}

unsafe extern "C" fn callback(app_state: *mut i8, data: *mut XRecordInterceptData) {
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

    unsafe {
        let main_display = XOpenDisplay(null());
        let record_display = XOpenDisplay(null());

        let mut app_state = AppState {
            main_display,
            _record_display: record_display,
            keys_down: Default::default(),
        };

        let mut clients = [XRecordAllClients];
        let range = XRecordAllocRange();
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
        let mut ranges = [range];
        let context = XRecordCreateContext(
            record_display,
            0,
            &mut clients[0],
            clients.len() as i32,
            &mut ranges[0],
            ranges.len() as i32,
        );
        let _result = XRecordEnableContext(
            record_display,
            context,
            Some(callback),
            &mut app_state as *mut _ as *mut i8,
        );
        println!("done");
        //assert_eq!(result, 0);
    }
}
