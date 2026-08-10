//! Infrastructure layer: stdin ingestion, terminal lifecycle, and the render
//! loop. Knows nothing about *what* is drawn — it drives any [`crate::display::Display`].

pub mod app;
pub mod feed;
pub mod input;
pub mod terminal;
