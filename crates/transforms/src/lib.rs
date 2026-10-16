//! Explicit, bounded public-data requests producing reviewable graph patches.
//! The service never mutates a graph or infers identity from matching labels.

mod parsers;
mod service;

pub use service::{Manifest, Output, Service};

#[cfg(test)]
mod tests;
