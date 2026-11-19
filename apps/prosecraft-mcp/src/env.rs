//! The clock and the random ids, behind a trait so tests can fix them.

use prosecraft_core::library::new_device_id;
use prosecraft_core::storage::crypto::OsRandom;

pub trait Env {
    /// `Date.now()`: milliseconds since the epoch.
    fn now(&mut self) -> f64;
    /// `crypto.randomUUID()`.
    fn uuid(&mut self) -> Result<String, String>;
}

/// The system clock and the operating system's random numbers.
#[derive(Debug, Default)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn now(&mut self) -> f64 {
        // Milliseconds fit an f64 exactly for the next quarter million years.
        chrono::Utc::now().timestamp_millis() as f64
    }

    fn uuid(&mut self) -> Result<String, String> {
        new_device_id(&mut OsRandom).map_err(|e| e.to_string())
    }
}
