//! Shared Rust telemetry core: wire model, bounds, clocks, providers, scopes and state.

pub mod bounds;
pub mod clock;
pub mod model;
pub mod providers;
pub mod rates;
pub mod scopes;
pub mod state;

pub mod baseline;
pub mod normalize;

pub mod history;
pub mod memory;
pub mod processes;
pub mod projection;

pub mod delivery;
pub mod inspection;
pub mod view;

pub mod collector;
