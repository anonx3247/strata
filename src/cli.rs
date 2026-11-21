//! Command-line options: a record that derives `std.cli.CommandLine` is read
//! from the program's arguments by its `from_args`, written in Strata
//! (`std/cli.strata`); this only says which types are read that way.

use crate::ast::*;
use crate::types::Env;

pub const TRAIT: &str = "std.cli.CommandLine";

/// Whether values of the named type are read from the command line.
pub fn is_options(env: &Env, ty: &TypeExpr) -> bool {
    let head = crate::types::texpr_head(ty);
    env.impls.iter().any(|i| i.trait_name == TRAIT && crate::types::texpr_head(&i.for_type) == head)
}

