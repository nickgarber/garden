extern crate shlex;

use ::cmd;
use ::eval;
use ::model;
use ::query;


pub fn main(app: &mut model::ApplicationContext) {
    let config = &mut app.config;
    let options = &mut app.options;

    let mut query = String::new();
    let mut tree = String::new();

    // Parse arguments
    {
        let mut ap = argparse::ArgumentParser::new();
        ap.set_description(
            "garden shell - open a shell in a garden environment");

        ap.refer(&mut query).required()
            .add_argument("query", argparse::Store,
                          "query for trees to build an environment");

        ap.refer(&mut tree)
            .add_argument("tree", argparse::Store, "tree to chdir into");

        options.args.insert(0, "garden shell".to_string());
        if let Err(err) = ap.parse(options.args.to_vec(),
                                   &mut std::io::stdout(),
                                   &mut std::io::stderr()) {
            std::process::exit(err);
        }
    }

    let contexts = query::resolve_trees(config, &query);
    if contexts.is_empty() {
        error!("tree query matched zero trees: '{}'", query);
    }

    let mut context = contexts[0].clone();

    // If a tree's name in the returned contexts exactly matches the tree
    // query that was used to find it then chdir into that tree.
    // This makes it convenient to have gardens and trees with the same name.
    for ctx in &contexts {
        if config.trees[ctx.tree].name == query {
            context.tree = ctx.tree;
            context.garden = ctx.garden;
            context.group = ctx.group;
            break;
        }
    }

    if !tree.is_empty() {
        let mut found = false;

        if let Some(ctx) = query::tree_from_name(config, &tree, None, None) {
            for query_ctx in &contexts {
                if ctx.tree == query_ctx.tree {
                    context.tree = query_ctx.tree;
                    context.garden = query_ctx.garden;
                    context.group = query_ctx.group;
                    found = true;
                    break;
                }
            }
        } else {
            error!("unable to find '{}': No tree exists with that name", tree);
        }
        if !found {
            error!("'{}' was not found in the tree query '{}'", tree, query);
        }
    }

    // Evaluate garden.shell
    let shell_expr = config.shell.to_string();
    let shell = eval::tree_value(config, &shell_expr,
                                 context.tree, context.garden);

    if let Some(value) = shlex::split(&shell) {
        let exit_status = cmd::exec_in_context(
            config, &context, /*quiet*/ true, /*verbose*/ false, &value);
        std::process::exit(exit_status);
    } else {
        error!("invalid configuration: unable to shlex::split '{}'", shell);
    }
}
