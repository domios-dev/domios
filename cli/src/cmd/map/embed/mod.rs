pub mod client;
pub mod path;

pub use client::{vec_to_le_bytes, EmbedClient};
pub use path::resolve_embed_path;

pub const VECTOR_DIM: usize = 384;
