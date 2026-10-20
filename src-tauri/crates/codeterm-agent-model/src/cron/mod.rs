pub mod clock;
pub mod store;
pub type CronState = std::sync::Arc<parking_lot::Mutex<store::CronStore>>;
