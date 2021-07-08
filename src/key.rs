#![allow(dead_code)]

use enumset::EnumSetType;
use libc::c_ulong;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::HashMap,
    convert::TryFrom,
    ffi::{CStr, CString},
    fmt::Debug,
    num::NonZeroU8,
    str::FromStr,
};
use x11::xlib::{NoSymbol, XKeysymToString, XStringToKeysym};

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Debug, Hash)]
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

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Debug, Hash)]
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

#[derive(Debug, EnumSetType, Deserialize, Serialize, Hash)]
#[enumset(serialize_as_list)]
#[serde(rename_all = "lowercase")]
pub enum Modifier {
    Shift = 0,
    CapsLock,
    Ctrl,
    Alt,
    NumLock,
    Mod3,
    Super,
    Mod5,
}

#[derive(Default)]
pub struct KeyboardMapping {
    keysym_to_keycodes: HashMap<Keysym, Vec<Keycode>>,
    keycode_to_keysym: HashMap<Keycode, Keysym>,
}

impl KeyboardMapping {
    pub fn insert(&mut self, keysym: Keysym, keycode: Keycode) {
        self.keysym_to_keycodes
            .entry(keysym)
            .or_default()
            .push(keycode);
        self.keycode_to_keysym.insert(keycode, keysym);
    }

    pub fn keysym_to_keycodes(&self, keysym: Keysym) -> Vec<Keycode> {
        self.keysym_to_keycodes
            .get(&keysym)
            .cloned()
            .unwrap_or_default()
    }

    pub fn _keycode_to_keysym(&self, keycode: Keycode) -> Option<Keysym> {
        self.keycode_to_keysym.get(&keycode).copied()
    }
}

#[derive(Default)]
pub struct ModifierMapping {
    keycode_to_modifier: HashMap<Keycode, Modifier>,
    modifier_to_keycodes: HashMap<Modifier, Vec<Keycode>>,
}

impl ModifierMapping {
    pub fn insert(&mut self, keycode: Keycode, modifier: Modifier) {
        self.keycode_to_modifier.insert(keycode, modifier);
        self.modifier_to_keycodes
            .entry(modifier)
            .or_default()
            .push(keycode);
    }

    pub fn keycode_to_modifier(&self, keycode: Keycode) -> Option<Modifier> {
        self.keycode_to_modifier.get(&keycode).copied()
    }

    pub fn modifier_to_keycodes(&self, modifier: Modifier) -> Vec<Keycode> {
        self.modifier_to_keycodes
            .get(&modifier)
            .cloned()
            .unwrap_or_default()
    }
}
