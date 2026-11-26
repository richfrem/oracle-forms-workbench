//! Purpose: Crate root module exports and public API surface.
//! Key Input Dependencies: patch, pld, reader, reports_parser, writer
//! Index: (module re-exports / constants)

pub mod patch;
pub mod pld;
pub mod reader;
pub mod reports_parser;
pub mod writer;

pub use patch::{patch_items, ItemPatch, PatchError};
pub use pld::PldParser;
pub use reader::FormsXmlParser;
pub use reports_parser::ReportsXmlParser;
pub use writer::FormsXmlWriter;
