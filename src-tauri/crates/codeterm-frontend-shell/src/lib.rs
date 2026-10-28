//! Frontend shell contract shared by the native client and the server: navigation origins and branded fallback pages.

mod brand_fallback;
mod development;
mod origins;

pub use brand_fallback::{brand_fallback_page, BrandFallback};
pub use development::{
    development_browser_arguments, development_runtime_frontend, DevelopmentFrontend,
};
pub use origins::{
    declared_frontend_origins, development_frontend, fallback_origin, loopback_origin,
};
