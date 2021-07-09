#![allow(dead_code)]

use enumset::EnumSet;
use serde::Deserialize;
use std::{convert::TryFrom, fs::File, io::prelude::*, path::PathBuf};

use crate::key::*;

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum KeySpec {
    Code(u8),
    Sym(String),
}

impl KeySpec {
    fn to_keycode(&self, keyboard_mapping: &KeyboardMapping) -> Keycode {
        match self {
            KeySpec::Code(c) => Keycode::try_from(*c as u8).expect("invalid keycode"),
            KeySpec::Sym(s) => {
                let keysym = s.parse().expect("invalid keysym");
                keyboard_mapping
                    .keysym_to_keycodes(keysym)
                    .get(0)
                    .copied()
                    .expect("no keysym for keycode")
            }
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum KeySeq {
    Key(KeySpec),
    Chord(Vec<KeySpec>),
    ChordSeq(Vec<Vec<KeySpec>>),
}

impl KeySeq {
    fn to_chord_seq(&self, keyboard_mapping: &KeyboardMapping) -> Vec<Vec<Keycode>> {
        match self.clone() {
            Self::Key(k) => Self::Chord(vec![k]).to_chord_seq(keyboard_mapping),
            Self::Chord(c) => Self::ChordSeq(vec![c]).to_chord_seq(keyboard_mapping),
            Self::ChordSeq(s) => s
                .into_iter()
                .map(|c| {
                    c.into_iter()
                        .map(|k| k.to_keycode(keyboard_mapping))
                        .collect()
                })
                .collect(),
        }
    }
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

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
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

const NUM_MODS: usize = 8;

impl ModSpec {
    pub fn required_set(&self) -> EnumSet<Modifier> {
        self.with_disposition(ModDisposition::Required)
    }

    pub fn allowed_set(&self) -> EnumSet<Modifier> {
        self.with_disposition(ModDisposition::Allowed)
    }

    pub fn forbidden_set(&self) -> EnumSet<Modifier> {
        self.with_disposition(ModDisposition::Forbidden)
    }

    fn combine_with(&self, other: &Self) -> Self {
        let mine = self.to_array();
        let theirs = other.to_array();
        let mut array = [ModDisposition::Allowed; NUM_MODS];
        for i in 0..NUM_MODS {
            array[i] = match (mine[i], theirs[i]) {
                (d1, d2) if d1 == d2 => d1,
                (ModDisposition::Allowed, d) => d,
                (d, ModDisposition::Allowed) => d,
                _ => panic!("invalid combination"),
            }
        }
        Self::from_slice(&array)
    }

    fn to_array(&self) -> [ModDisposition; NUM_MODS] {
        [
            self.shift,
            self.capslock,
            self.ctrl,
            self.alt,
            self.numlock,
            self.mod3,
            self.super_,
            self.mod5,
        ]
    }

    fn from_slice(slice: &[ModDisposition; NUM_MODS]) -> Self {
        Self {
            shift: slice[0],
            capslock: slice[1],
            ctrl: slice[2],
            alt: slice[3],
            numlock: slice[4],
            mod3: slice[5],
            super_: slice[6],
            mod5: slice[7],
        }
    }

    pub fn matches(&self, modifiers: EnumSet<Modifier>) -> bool {
        for modifier in EnumSet::all() {
            match (self.disposition_of(modifier), modifiers.contains(modifier)) {
                (ModDisposition::Required, false) | (ModDisposition::Forbidden, true) => {
                    return false
                }
                _ => {}
            }
        }
        true
    }

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

    fn disposition_of(&self, modifier: Modifier) -> ModDisposition {
        self.to_array()[modifier as usize]
    }

    fn with_disposition(&self, disposition: ModDisposition) -> EnumSet<Modifier> {
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
    pub input: KeySpec,
    pub output: KeySeq,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Conditions {
    pub window_title: Option<String>,
}

impl Conditions {
    fn combine_with<'a>(&self, other: &Self) -> Self {
        if self.window_title.is_none() {
            other.clone()
        } else if other.window_title.is_none() {
            self.clone()
        } else {
            unimplemented!("can't combine conditions yet")
        }
    }
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
    pub mods: ModSpec,
    #[serde(flatten)]
    body: ItemBody,
}

impl ConfigItem {
    pub fn visit_key_mappings<F>(&self, state: VisitKeyMappingsState, f: &mut F) -> ControlFlow
    where
        F: FnMut(&KeyMapping, VisitKeyMappingsState) -> ControlFlow,
    {
        let conditions = state.conditions.combine_with(&self.conditions);
        let mods = state.mods.combine_with(&self.mods);
        let state = VisitKeyMappingsState { conditions, mods };

        if self.enabled {
            match &self.body {
                ItemBody::KeyMapping(m) => {
                    if f(m, state.clone()) == ControlFlow::Break {
                        return ControlFlow::Break;
                    }
                }
                ItemBody::Group { contents } => {
                    for item in contents {
                        if item.visit_key_mappings(state.clone(), f) == ControlFlow::Break {
                            return ControlFlow::Break;
                        }
                    }
                }
            }
        }
        ControlFlow::Continue
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ItemBody {
    KeyMapping(KeyMapping),
    Group { contents: Vec<ConfigItem> },
}

#[derive(Clone, Default)]
pub struct VisitKeyMappingsState {
    mods: ModSpec,
    conditions: Conditions,
}

#[derive(Debug, Deserialize, Default)]
pub struct Config(Vec<ConfigItem>);

impl Config {
    pub fn load(path: Option<PathBuf>) -> Result<Self, String> {
        let path = path
            .or_else(|| {
                let msg = "error finding config file";
                xdg::BaseDirectories::with_prefix("autokey-rs")
                    .expect(msg)
                    .find_config_file("config.json5")
            })
            .ok_or("no config path specified".to_string())?;
        let mut config_data = File::open(path.clone()).map_err(|err| {
            format!(
                "error opening config file: {}: {}",
                path.to_string_lossy(),
                err
            )
        })?;
        let mut config_buf = String::new();
        config_data.read_to_string(&mut config_buf).map_err(|err| {
            format!(
                "error reading config file: {}: {}",
                path.to_string_lossy(),
                err
            )
        })?;
        json5::from_str(&config_buf).map_err(|err| {
            format!(
                "error parsing config file: {}: {}",
                path.to_string_lossy(),
                err
            )
        })
    }

    pub fn visit_key_mappings<F>(&self, f: &mut F) -> ControlFlow
    where
        F: FnMut(&KeyMapping, VisitKeyMappingsState) -> ControlFlow,
    {
        self.visit_key_mappings_with_state(Default::default(), f)
    }

    fn visit_key_mappings_with_state<F>(
        &self,
        state: VisitKeyMappingsState,
        f: &mut F,
    ) -> ControlFlow
    where
        F: FnMut(&KeyMapping, VisitKeyMappingsState) -> ControlFlow,
    {
        for item in &self.0 {
            if item.visit_key_mappings(state.clone(), f) == ControlFlow::Break {
                return ControlFlow::Break;
            }
        }
        ControlFlow::Continue
    }

    pub fn validate(&self, keyboard_mapping: &KeyboardMapping) -> ValidConfig {
        let mut valid = ValidConfig {
            key_mappings: Default::default(),
        };
        self.visit_key_mappings(&mut |k, state| {
            let input = k.input.to_keycode(keyboard_mapping);
            let output = k.output.to_chord_seq(keyboard_mapping);
            valid.key_mappings.push(ValidKeyMapping {
                input,
                output,
                conditions: state.conditions,
                mods: state.mods,
            });
            ControlFlow::Continue
        });
        valid
    }
}

#[derive(PartialEq, Eq)]
pub enum ControlFlow {
    Continue,
    Break,
}

pub struct ValidConfig {
    pub key_mappings: Vec<ValidKeyMapping>,
}

pub struct ValidKeyMapping {
    pub conditions: Conditions,
    pub mods: ModSpec,
    pub input: Keycode,
    pub output: Vec<Vec<Keycode>>,
}
