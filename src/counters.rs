use std::sync::{
    OnceLock,
    atomic::{AtomicU16, Ordering},
};

/// A single counter.
struct Counter {
    crate_name: String,
    id: String,

    counter: AtomicU16,
}

/// Stores counters referred to by id strings.  
/// Will never deadlock.
pub struct Counters(
    // 5 per allocation was chosen arbitrarily.
    [OnceLock<Counter>; 5],
    OnceLock<Box<Counters>>,
);

impl Counters {
    /// Creates an empty set of counters.
    pub const fn new() -> Self {
        Self([const { OnceLock::new() }; 5], OnceLock::new())
    }

    /// Adds to the current value, returning the previous value.\
    /// If the counter with the specified id does not exist, it will start from `start_at`.\
    /// Returns an Err([`std::env::VarError`]) if `CARGO_CRATE_NAME` couldn't be fetched from the environment.
    pub fn fetch_add(&self, id: String, start_at: u16) -> Result<u16, std::env::VarError> {
        let crate_name: String = std::env::var("CARGO_CRATE_NAME")?;

        Ok(self.fetch_add_internal(crate_name, id, start_at))
    }

    /// Recursive interior of `fetch_add`.
    fn fetch_add_internal(&self, crate_name: String, id: String, start_at: u16) -> u16 {
        for trait_counter in &self.0 {
            let trait_counter = trait_counter.get_or_init(|| Counter {
                crate_name: crate_name.clone(),
                id: id.clone(),

                counter: start_at.into(),
            });

            if trait_counter.crate_name == crate_name && trait_counter.id == id {
                return trait_counter.counter.fetch_add(1, Ordering::Relaxed);
            }
        }

        // If the counter couldn't be found in these counters, then we try again in the next lot of counters.
        let next = self.1.get_or_init(|| Box::new(Counters::new()));
        next.fetch_add_internal(crate_name, id, start_at)
    }
}
