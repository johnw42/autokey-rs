pub struct Config {
    pub rules: Vec<Rule>,
}

pub enum Rule {
    Group(Vec<Rule>),
}

pub struct InputKey {}

enum KeySpec {
    Code(i32),
    Sym(String),
}
