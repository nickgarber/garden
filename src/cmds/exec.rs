extern crate subprocess;

use super::super::cmd;
use super::super::eval;
use super::super::model;
use super::super::query;

/// Resolve garden and tree names into a set of trees
/// Strategy: resolve the trees down to a set of tree indexes paired with an
/// an optional garden context.
///
/// If the names resolve to gardens, each garden is processed independently.
/// Trees that exist in multiple matching gardens will be processed multiple
/// times.
///
/// If the names resolve to trees, each tree is processed independently
/// with no garden context.

pub fn main<S>(
    config: &mut model::Configuration,
    quiet: bool,
    verbose: bool,
    expr: S,
    command: &Vec<String>,
) where S: Into<String> {

    // Resolve the tree expression into a vector of tree contexts.
    let contexts = query::resolve_trees(config, expr);
    let mut exit_status: i32 = 0;

    // Loop over each context, evaluate the tree environment,
    // and run the command.
    for context in &contexts {
        // Evaluate the tree environment
        let env = eval::environment(config, context);

        // Exec each command in the tree's context
        let tree = &config.trees[context.tree];
        if !quiet {
            if verbose {
                eprintln!("# {}: {}", tree.name,
                          tree.path.value.as_ref().unwrap());
            } else {
                eprintln!("# {}", tree.name);
            }
        }

        let path = tree.path.value.as_ref().unwrap();
        let mut exec = subprocess::Exec::cmd(command[0].to_string())
            .args(&command[1..])
            .cwd(path);

        // Update the command environment
        for (name, value) in &env {
            exec = exec.env(name, value);
        }

        let status = cmd::status(exec.join());
        if status != 0 {
            exit_status = status as i32;
        }
    }

    // Return the last non-zero exit status.
    std::process::exit(exit_status);
}