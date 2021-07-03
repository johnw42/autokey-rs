#![allow(non_snake_case)]

use std::{error::Error, ptr::null_mut, sync::Mutex};

use libc::c_void;
use xcb::{
    ffi::{
        record::{
            xcb_record_enable_context_data, xcb_record_enable_context_data_length,
            xcb_record_enable_context_reply_t,
        },
        xcb_generate_id, xcb_key_press_event_t,
    },
    query_extension,
    record::{
        create_context, enable_context, free_context, EnableContextCookie, ExtRange, Range,
        Range16, Range8, CS_ALL_CLIENTS,
    },
    Connection, CookieSeq, BUTTON_PRESS, KEY_PRESS,
};

use crate::xcb_ext;

pub struct XRecordInterface {
    recordDisplay: Connection,
}

impl XRecordInterface {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let (recordDisplay, _) = xcb::base::Connection::connect(None)?;
        assert!(query_extension(&recordDisplay, "RECORD")
            .get_reply()?
            .present());

        let context = recordDisplay.generate_id();
        let element_header = 0;
        let client_specs = [CS_ALL_CLIENTS];
        let ranges = [Range::new(
            Range8::new(0, 0),
            Range8::new(0, 0),
            ExtRange::new(Range8::new(0, 0), Range16::new(0, 0)),
            ExtRange::new(Range8::new(0, 0), Range16::new(0, 0)),
            Range8::new(0, 0),
            Range8::new(KEY_PRESS, BUTTON_PRESS),
            Range8::new(0, 0),
            false,
            false,
        )];
        create_context(
            &recordDisplay,
            context,
            element_header,
            &client_specs,
            &ranges,
        )
        .request_check()?;
        let cookie = enable_context(&recordDisplay, context);
        let mut reply = null_mut();
        let mut error = null_mut();
        unsafe {
            loop {
                if xcb_ext::xcb_poll_for_reply(
                    recordDisplay.get_raw_conn(),
                    cookie.cookie.sequence(),
                    &mut reply,
                    &mut error,
                ) != 0
                {
                    let reply = reply as *mut xcb_record_enable_context_reply_t;
                    let events =
                        xcb_record_enable_context_data(reply) as *const xcb_key_press_event_t;
                    let num_events = xcb_record_enable_context_data_length(reply);
                    println!("num events: {}", num_events);
                    for i in 0..num_events {
                        let event = &*events.add(i as usize);
                        match event.response_type {
                            xcb::xproto::KEY_PRESS => {
                                println!("key press");
                            }
                            xcb::xproto::KEY_RELEASE => {
                                println!("rey release");
                            }
                            xcb::xproto::BUTTON_PRESS => {
                                println!("button press");
                            }
                            _ => {}
                        }
                    }
                    libc::free(reply as *mut c_void);
                }
            }
        }
        //free_context(&recordDisplay, context).request_check()?;

        //Ok(Self { recordDisplay })
        panic!()
    }
}
