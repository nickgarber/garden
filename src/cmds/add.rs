extern crate subprocess;
extern crate yaml_rust;

use std::io::Write;

use self::yaml_rust::YamlEmitter;
use self::yaml_rust::YamlLoader;
use self::yaml_rust::yaml;

use ::cmd;
use ::config;
use ::model;


pub fn main(options: &mut model::CommandOptions) {
    // Parse arguments
    options.args.insert(0, "garden add".to_string());
    let mut output = String::new();
    let mut paths: Vec<String> = Vec::new();
    {
        let mut ap = argparse::ArgumentParser::new();
        ap.set_description("add existing trees to a garden configuration");

        ap.refer(&mut output)
            .add_option(&["-o", "--output"], argparse::Store,
                        "file to write (defaults to the config file)");

        ap.refer(&mut paths).required()
            .add_argument("paths", argparse::List, "trees to add");

        if let Err(err) = ap.parse(options.args.to_vec(),
                                   &mut std::io::stdout(),
                                   &mut std::io::stderr()) {
            std::process::exit(err);
        }
    }

    let verbose = options.is_debug("config::new");
    let cfg = config::new(&options.filename, verbose);
    if cfg.path.is_none() {
        error!("unable to find a configuration file -- use --config <path>");
    }
    if options.is_debug("config") {
        debug!("{}", cfg);
    }
    if options.verbose {
        eprintln!("config: {:?}", cfg.path.as_ref().unwrap());
    }

    // Read existing configuration
    let mut doc = match yaml_from_path(cfg.path.as_ref().unwrap()) {
        Ok(doc) => {
            doc
        }
        Err(err) => {
            error!("{}", err);
            return;
        }
    };

    // Output filename defaults to the input filename.
    if output.is_empty() {
        output = cfg.path.as_ref().unwrap().to_string_lossy().to_string();
    }

    {
        // Get a mutable reference to top-level document hash.
        let doc_hash: &mut yaml::Hash = match doc {
            yaml::Yaml::Hash(ref mut hash) => {
                hash
            },
            _ => {
                error!("invalid config: not a hash");
                return;
            },
        };

        // Get a mutable reference to the "trees" hash.
        let key = yaml::Yaml::String("trees".to_string());
        let trees: &mut yaml::Hash = match doc_hash.get_mut(&key) {
            Some(yaml::Yaml::Hash(ref mut hash)) => {
                hash
            },
            _ => {
                error!("invalid trees: not a hash");
                return;
            }
        };

        for path in &paths {
            if let Err(msg) = add_path(&cfg, options.verbose, path, trees) {
                error!("{}", msg);
            }
        }
    }

    // Emit the YAML configuration into a string
    let mut out_str = String::new();
    {
        let mut emitter = YamlEmitter::new(&mut out_str);
        emitter.dump(&doc).unwrap(); // dump the YAML object to a String
    }
    out_str += "\n";

    let file_result = std::fs::File::create(&output);
    if file_result.is_err() {
        error!("{}: unable to create configuration: {}",
               output, file_result.as_ref().err().unwrap());
    }

    let mut file = file_result.unwrap();
    let write_result = file.write_all(&out_str.into_bytes());
    if write_result.is_err() {
        error!("{}: unable to write configuration", output);
    }

    if let Err(err) = file.sync_all() {
        error!("unable to sync files: {}", err);
    }
}


fn yaml_from_path(path: &std::path::PathBuf) -> Result<yaml::Yaml, String> {
    // Read existing configuration
    let read_result = std::fs::read_to_string(path);
    if read_result.is_err() {
        return Err(format!(
            "unable to read {:?}: {:?}", path, read_result.err()));
    }

    let string = read_result.unwrap();
    let docs_result = YamlLoader::load_from_str(&string);
    if docs_result.is_err() {
        return Err(format!(
            "unable to read configuration: {:?}", docs_result.err()));
    }

    let mut docs = docs_result.unwrap();
    if docs.len() < 1 {
        return Err(format!("empty configuration: {:?}", path));
    }

    add_trees_if_missing(&mut docs[0]);
    Ok(docs[0].clone())
}

fn add_trees_if_missing(doc: &mut yaml::Yaml) {
    let good = doc["trees"].as_hash().is_some();
    if !good {
        if let yaml::Yaml::Hash(ref mut doc_hash) = doc {
            let key = yaml::Yaml::String("trees".to_string());
            doc_hash.remove(&key);
            doc_hash.insert(key, yaml::Yaml::Hash(yaml::Hash::new()));
        } else {
            error!("invalid configuration format: not a hash");
        }
    }
}


fn add_path(
    config: &model::Configuration,
    verbose: bool,
    raw_path: &str,
    trees: &mut yaml::Hash)
-> Result<(), String> {

    // Garden root path
    let root = config.root_path.canonicalize().unwrap().to_path_buf();

    let pathbuf = std::path::PathBuf::from(raw_path);
    if !pathbuf.exists() {
        return Err(format!("{}: invalid tree path", raw_path));
    }

    // Build the tree's path
    let tree_path: String;

    // Get a canonical tree path for comparison with the canonical root.
    let path = pathbuf.canonicalize().unwrap().to_path_buf();

    // Is the path a child of the current garden root?
    if path.starts_with(&root) && path.strip_prefix(&root).is_ok() {

        tree_path = path
            .strip_prefix(&root).unwrap().to_string_lossy().to_string();
    } else {
        tree_path = path.to_string_lossy().to_string();
    }

    // Tree name is updated when an existing tree is found.
    let mut tree_name = tree_path.to_string();

    // Do we already have a tree with this tree path?
    for tree in &config.trees {
        let cfg_tree_path_result = std::path::PathBuf::from(
            tree.path.value.as_ref().unwrap()).canonicalize();
        if cfg_tree_path_result.is_err() {
            continue;  // skip missing entries
        }

        let cfg_tree_path = cfg_tree_path_result.unwrap();
        if cfg_tree_path == path {
            // Tree found: take its configured name.
            tree_name = tree.name.to_string();
        }
    }

    // Key for the tree entry
    let key = yaml::Yaml::String(tree_name.to_string());
    let mut entry: yaml::Hash = yaml::Hash::new();

    // Update an existing entry if it already exists.
    // Add a new entry otherwise.
    if trees.get(&key).is_some()
    && trees.get(&key).unwrap().as_hash().is_some() {
        entry = trees.get(&key).unwrap().as_hash().unwrap().clone();
        if verbose {
            eprintln!("{}: found existing tree", tree_name);
        }
    }

    let remotes_key = yaml::Yaml::String("remotes".to_string());
    let has_remotes = entry.contains_key(&remotes_key)
        && entry.get(&remotes_key).unwrap().as_hash().is_some();

    // Gather remote names
    let mut remote_names: Vec<String> = Vec::new();
    {
        let command = ["git", "remote"];
        let exec = subprocess::Exec::cmd(&command[0])
            .args(&command[1..]).cwd(&path);

        if let Ok(x) = cmd::capture_stdout(exec) {
            let output = cmd::trim_stdout(&x);

            for line in output.lines() {
                // Skip "origin" since it is defined by the "url" entry.
                if line == "origin" {
                    continue;
                }
                // Any other remotes are part of the "remotes" hash.
                remote_names.push(line.to_string());
            }
        }
    }

    // Gather remote urls
    let mut remotes: Vec<(String, String)> = Vec::new();
    {
        for remote in &remote_names {
            let mut command: Vec<String> = Vec::new();
            command.push("git".into());
            command.push("config".into());
            command.push("remote.".to_string() + remote + ".url");

            let exec = subprocess::Exec::cmd(&command[0])
                .args(&command[1..]).cwd(&path);

            if let Ok(x) = cmd::capture_stdout(exec) {
                let output = cmd::trim_stdout(&x);
                remotes.push((remote.to_string(), output));
            }
        }
    }

    if !remotes.is_empty() {
        if !has_remotes {
            entry.insert(remotes_key.clone(), yaml::Yaml::Hash(yaml::Hash::new()));
        }

        let remotes_hash: &mut yaml::Hash =
            match entry.get_mut(&remotes_key) {
            Some(yaml::Yaml::Hash(ref mut hash)) => {
                hash
            },
            _ => {
                return Err("trees: not a hash".to_string());
            }
        };

        for (k, v) in &remotes {
            let remote = yaml::Yaml::String(k.to_string());
            let value = yaml::Yaml::String(v.to_string());

            if remotes_hash.contains_key(&remote) {
                *(remotes_hash.get_mut(&remote).unwrap()) = value;
            } else {
                remotes_hash.insert(remote, value);
            }
        }
    }

    let url_key = yaml::Yaml::String("url".to_string());
    let has_url = entry.contains_key(&url_key);
    if !has_url {
        if verbose {
            eprintln!("{}: no url", tree_name);
        }

        let command = ["git", "config", "remote.origin.url"];
        let exec = subprocess::Exec::cmd(&command[0])
            .args(&command[1..]).cwd(&path);

        match cmd::capture_stdout(exec) {
            Ok(x) => {
                let origin_url = cmd::trim_stdout(&x);
                entry.insert(url_key, yaml::Yaml::String(origin_url));
            }
            Err(err) => {
                error!("{:?}", err);
            }
        }
    }

    // Move the entry into the trees container
    if trees.contains_key(&key) {
        *(trees.get_mut(&key).unwrap()) = yaml::Yaml::Hash(entry);
    } else {
        trees.insert(key, yaml::Yaml::Hash(entry));
    }

    Ok(())
}
