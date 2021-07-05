use enumset::EnumSet;
use serde::Deserialize;

use crate::key::*;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum KeySpec {
    Code(i32),
    Sym(String),
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[serde(from = "BoolModDisposition")]
pub enum ModDisposition {
    Forbidden,
    Allowed,
    Required,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BoolModDisposition {
    Bool(bool),
    ModDisposition(ModDisposition),
}

impl From<BoolModDisposition> for ModDisposition {
    fn from(d: BoolModDisposition) -> Self {
        match d {
            BoolModDisposition::Bool(true) => ModDisposition::Required,
            BoolModDisposition::Bool(false) => ModDisposition::Forbidden,
            BoolModDisposition::ModDisposition(d) => d,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ModSpec {
    pub shift: ModDisposition,
    pub capslock: ModDisposition,
    #[serde(alias = "ctrl")]
    pub ctrl: ModDisposition,
    pub alt: ModDisposition,
    pub numlock: ModDisposition,
    pub mod3: ModDisposition,
    #[serde(rename = "super", alias = "win")]
    pub super_: ModDisposition,
    pub mod5: ModDisposition,
}

impl ModSpec {
    pub fn disposition_of(&self, modifier: Modifier) -> ModDisposition {
        match modifier {
            Modifier::Shift => self.shift,
            Modifier::CapsLock => self.capslock,
            Modifier::Ctrl => self.ctrl,
            Modifier::Alt => self.alt,
            Modifier::NumLock => self.numlock,
            Modifier::Mod3 => self.mod3,
            Modifier::Super => self.super_,
            Modifier::Mod5 => self.mod5,
        }
    }

    pub fn with_disposition(&self, disposition: ModDisposition) -> EnumSet<Modifier> {
        EnumSet::<Modifier>::all()
            .into_iter()
            .filter(|m| self.disposition_of(*m) == disposition)
            .collect()
    }

    pub fn required(&self) -> EnumSet<Modifier> {
        self.with_disposition(ModDisposition::Required)
    }
}

impl Default for ModSpec {
    fn default() -> Self {
        Self {
            shift: ModDisposition::Allowed,
            capslock: ModDisposition::Allowed,
            ctrl: ModDisposition::Allowed,
            alt: ModDisposition::Allowed,
            numlock: ModDisposition::Allowed,
            mod3: ModDisposition::Allowed,
            super_: ModDisposition::Allowed,
            mod5: ModDisposition::Allowed,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct KeyMapping {
    #[serde(flatten)]
    pub conditions: Conditions,
    #[serde(flatten)]
    pub mods: ModSpec,
    pub input: KeySpec,
    pub output: Vec<Vec<KeySpec>>,
}

impl KeyMapping {
    fn visit_keyspecs<F>(&mut self, f: F)
    where
        F: Fn(&mut KeySpec),
    {
        f(&mut self.input);
        self.output.iter_mut().flatten().for_each(|k| f(k))
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct Conditions {
    pub window_title: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Group {
    #[serde(flatten)]
    pub conditions: Conditions,
    pub contents: Vec<Item>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Item {
    KeyMapping(KeyMapping),
    Group(Group),
}

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub items: Vec<Item>,
}

impl Config {
    pub fn visit_keyspecs<F>(&mut self, f: F)
    where
        F: Fn(&mut KeySpec),
    {
        //self.mappings.iter_mut().for_each(|m| m.visit_keyspecs(&f))
    }
}
