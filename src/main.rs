// mod x_interface;
// mod xcb_ext;

// use x_interface::*;

use std::mem::size_of_val;
use std::os::raw::c_int;
use std::ptr::{null, null_mut};
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

#[allow(non_upper_case_globals)]
unsafe extern "C" fn callback(_closure: *mut i8, data: *mut XRecordInterceptData) {
    let data = &mut *data;
    if data.category != XRecordFromServer || data.client_swapped != 0 || data.data_len == 0 {
        return;
    }

    let event = &*(data.data as *const RawEvent);
    debug_assert_eq!((data.data_len * 4) as usize, size_of_val(event));
    match event.code as c_int {
        KeyPress => {
            println!("KeyPress: {} {:x}", event.detail, event.state);
        }
        KeyRelease => {
            println!("KeyPress: {} {:x}", event.detail, event.state);
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
        let display = XOpenDisplay(null());
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
            display,
            0,
            &mut clients[0],
            clients.len() as i32,
            &mut ranges[0],
            ranges.len() as i32,
        );
        let _result = XRecordEnableContext(display, context, Some(callback), null_mut());
        //assert_eq!(result, 0);
    }
}
