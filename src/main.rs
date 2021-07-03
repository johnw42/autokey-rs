// mod x_interface;
// mod xcb_ext;

// use x_interface::*;

use libc::{c_int, c_uchar};
use std::ffi::{CStr, CString};
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

struct AppState {
    main_display: *mut Display,
    record_display: *mut Display,
}

impl AppState {
    unsafe fn write_key(&self, label: &str, keycode: c_uchar, state: u16) {
        let keysym = XKeycodeToKeysym(self.main_display, keycode, 0);
        let keysym_str = XKeysymToString(keysym);
        let keysym_str = (!keysym_str.is_null()).then(|| CStr::from_ptr(keysym_str).to_owned());
        println!(
            "{}: code={}, sym={} ({:?}), state={}",
            label,
            keycode,
            keysym,
            keysym_str.unwrap_or_else(|| CString::new("???").expect("conversion failed")),
            state
        );
    }
}

#[allow(non_upper_case_globals)]
unsafe extern "C" fn callback(app_state: *mut i8, data: *mut XRecordInterceptData) {
    let app_state = &mut *(app_state as *mut AppState);
    let data = &mut *data;
    if data.category != XRecordFromServer || data.client_swapped != 0 || data.data_len == 0 {
        return;
    }

    let event = &*(data.data as *const RawEvent);
    debug_assert_eq!((data.data_len * 4) as usize, size_of_val(event));
    match event.code as c_int {
        KeyPress => {
            app_state.write_key("KeyPress", event.detail, event.state);
        }
        KeyRelease => {
            app_state.write_key("KeyRelease", event.detail, event.state);
        }
        ButtonPress => {
            println!("ButtonPress: {} {:x}", event.detail, event.state);
        }
        _ => {
            println!("event type: {:?}", event);
        }
    }

    XRecordFreeData(data as *mut _);
}

fn main() {
    println!("Hello, world!");

    unsafe {
        let main_display = XOpenDisplay(null());
        let record_display = XOpenDisplay(null());

        let mut app_state = AppState {
            main_display,
            record_display,
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
