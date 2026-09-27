//! `lox tui` — interactive terminal UI (docs/design-tui.md).
// Render helpers take a buffer, an area, a position and styling; bundling
// those into structs would only move the argument list elsewhere.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod app;
pub mod data;
pub mod demo;
pub mod exec;
pub mod keymap;
pub mod lists;
pub mod model;
pub mod palette;
pub mod run;
pub mod store;
#[cfg(test)]
mod tests;
pub mod text;
pub mod theme;
pub mod ui;
pub mod update;
pub mod vm;
pub mod widgets;
