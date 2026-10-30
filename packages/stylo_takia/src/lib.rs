//! Conversion functions from Stylo types to Takia types
//!
//! This crate is an implementation detail of [`apace-dom`](https://docs.rs/apace-dom), but can also be
//! used standalone, and serves as useful reference for anyone wanting to integrate [`stylo`](::style) with [`takia`]

mod copy_getters;
pub use copy_getters::CopyStyleGetters;

mod wrapper;
pub use wrapper::{StyleFlags, TakiaStyloStyle};

pub mod convert;
#[doc(inline)]
pub use convert::to_takia_style;
#[doc(inline)]
pub use convert::to_takia_style_in;

pub mod writing_mode;
pub use style::logical_geometry::WritingMode;
pub use writing_mode::WritingModeExt;

pub use style::Atom;
