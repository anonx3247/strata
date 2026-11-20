//! The commands, one function each; `main` dispatches to them.

mod build;
mod check;
mod codebase;
mod git;
mod run;
mod views;

pub use build::*;
pub use check::*;
pub use codebase::*;
pub use git::*;
pub use run::*;
pub use views::*;
