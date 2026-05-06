pub mod http;
pub mod tauri;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handler {
    pub name: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invoke {
    pub command: String,
    pub line: u32,
}
