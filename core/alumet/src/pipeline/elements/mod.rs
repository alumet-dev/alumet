//! Implementation of the logic of pipeline elements.
//!
//! # Why are the builders traits?
//! Builders are just closures, but they are quite long and used in various places of the `alumet` crate.
//! To deduplicate the code and make it more readable, _trait aliases_ would have been idea.
//!
//! Unfortunately, _trait aliases_ are currently unstable.
//! Therefore, I have defined subtraits with an automatic implementation for closures.
pub mod error;
pub mod output;
pub mod source;
pub mod transform;
