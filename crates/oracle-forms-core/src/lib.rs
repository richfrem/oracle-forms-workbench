//! Purpose: Crate root module exports and public API surface.
//! Key Input Dependencies: diff, error, model, report, sidecars
//! Index: (module re-exports / constants)

pub mod diff;
pub mod error;
pub mod geometry;
pub mod model;
pub mod report;
pub mod sidecars;

pub use diff::{BlockDiff, FormDiffer, ItemDiff, ItemGeometryDiff, ModuleDiff, PropertyChange, TriggerDiff};
pub use error::{FormsError, Result};
pub use model::*;
pub use report::*;
pub use sidecars::PlsqlSidecars;
