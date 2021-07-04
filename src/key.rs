use std::{
    borrow::Cow,
    convert::TryFrom,
    ffi::{CStr, CString},
    fmt::Debug,
    num::NonZeroU8,
    str::FromStr,
};

use libc::c_ulong;
use x11::xlib::{NoSymbol, XKeysymToString, XStringToKeysym};

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Debug)]
pub struct Keycode(NonZeroU8);

impl Keycode {
    pub fn value(&self) -> u8 {
        self.0.get()
    }
}

impl TryFrom<u8> for Keycode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        NonZeroU8::new(value).map(Keycode).ok_or(())
    }
}

impl PartialEq<u8> for Keycode {
    fn eq(&self, other: &u8) -> bool {
        self.value() == *other
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Debug)]
pub struct Keysym(c_ulong);

impl Keysym {
    pub fn value(&self) -> c_ulong {
        self.0
    }

    pub fn to_c_str(&self) -> Option<&'static CStr> {
        unsafe {
            XKeysymToString(self.0)
                .as_ref()
                .map(|ptr| CStr::from_ptr(ptr))
        }
    }

    pub fn to_string(&self) -> Option<Cow<'static, str>> {
        self.to_c_str().map(|s| s.to_string_lossy())
    }
}

impl From<c_ulong> for Keysym {
    fn from(n: c_ulong) -> Self {
        Self(n)
    }
}

impl FromStr for Keysym {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cstr = CString::new(s).expect("invalid keysym string");
        let n = unsafe { XStringToKeysym(cstr.as_ptr()) };
        if n == NoSymbol as c_ulong {
            Err(())
        } else {
            Ok(Keysym(n))
        }
    }
}

#[derive(Clone, Copy)]
pub struct Modifier(i32);

const MODIFIER_NAMES: [&str; 8] = [
    "Shift", "CapsLock", "Ctrl", "Alt", "NumLock", "Mod3", "Super", "Mod5",
];

impl Modifier {
    pub fn values() -> Vec<Modifier> {
        (0..8).map(Modifier).collect()
    }

    fn to_str(self) -> &'static str {
        MODIFIER_NAMES[self.0 as usize]
    }
}

impl Debug for Modifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_str())
    }
}
