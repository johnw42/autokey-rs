#![allow(dead_code)]

use enumset::EnumSet;
use serde::Deserialize;

use crate::key::*;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum KeySpec {
    Code(i32),
    Sym(String),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum KeySeq {
    Key(KeySpec),
    Chord(Vec<KeySpec>),
    ChordSeq(Vec<Vec<KeySpec>>),
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
    pub fn mod_sets(&self) -> Vec<EnumSet<Modifier>> {
        let required_set = self.with_disposition(ModDisposition::Required);
        let allowed_set = self.with_disposition(ModDisposition::Allowed);
        let mut result = Vec::with_capacity(1 << allowed_set.len());
        result.push(required_set);
        for new_mod in allowed_set {
            for i in 0..result.len() {
                let old_set = result[i];
                debug_assert!(!old_set.contains(new_mod));
                result.push(old_set | new_mod);
            }
        }
        result
    }

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
    pub output: KeySeq,
}

#[derive(Debug, Deserialize, Default)]
pub struct Conditions {
    pub window_title: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct ConfigItem {
    name: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(flatten)]
    pub conditions: Conditions,
    #[serde(flatten)]
    body: ItemBody,
}

impl ConfigItem {
    pub fn visit_key_mappings<F>(&self, f: &F)
    where
        F: Fn(&KeyMapping),
    {
        if self.enabled {
            match &self.body {
                ItemBody::KeyMapping(m) => f(m),
                ItemBody::Group { contents } => {
                    contents.iter().for_each(|item| item.visit_key_mappings(f))
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ItemBody {
    KeyMapping(KeyMapping),
    Group { contents: Vec<ConfigItem> },
}

#[derive(Debug, Deserialize, Default)]
pub struct Config(Vec<ConfigItem>);

impl Config {
    pub fn visit_key_mappings<F>(&self, f: &F)
    where
        F: Fn(&KeyMapping),
    {
        self.0.iter().for_each(|item| item.visit_key_mappings(f))
    }
}
