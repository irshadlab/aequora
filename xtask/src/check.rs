use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use regex::Regex;
use serde::Deserialize;

use crate::{Result, cargo, ensure, failure, load_ron, output, successful, supply_chain};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum Check {
    File(PathBuf),
    Directory(PathBuf),
    Match {
        pattern: String,
        paths: Vec<PathBuf>,
        #[serde(default)]
        forbidden: bool,
        #[serde(default)]
        literal: bool,
    },
    Cargo {
        args: Vec<String>,
        #[serde(default)]
        quiet: bool,
        #[serde(default)]
        status: i32,
        #[serde(default)]
        require: Vec<String>,
        #[serde(default)]
        forbid: Vec<String>,
    },
    FileCount {
        directory: PathBuf,
        extension: String,
        minimum: usize,
        #[serde(default)]
        maximum: Option<usize>,
    },
    LineLimit {
        path: PathBuf,
        maximum: usize,
    },
    PinnedActions,
    TestkitDependents,
    SupplyChain,
    DatabaseProfiles,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DatabaseProfile {
    name: String,
    features: Vec<String>,
    require: Vec<String>,
    forbid: Vec<String>,
}

pub(super) fn run(root: &Path, args: &[String]) -> Result {
    let directory = root.join("architecture/checks");
    let mut names = Vec::new();
    for entry in fs::read_dir(&directory)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "ron") {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| failure(format!("invalid check filename {}", path.display())))?;
            names.push(name.to_owned());
        }
    }
    names.sort_unstable();
    match args {
        [name] if name == "list" => {
            for name in names {
                println!("{name}");
            }
            return Ok(());
        }
        [_] | [_, _] => {}
        _ => {
            return Err(failure(
                "usage: cargo xtask check <name|all|list> [--static]",
            ));
        }
    }
    let name = &args[0];
    let static_only = args.get(1).is_some_and(|flag| flag == "--static");
    ensure(args.len() == 1 || static_only, "unknown check option")?;
    ensure(
        name == "all" || names.contains(name),
        format!("unknown check {name}"),
    )?;
    let mut failures = Vec::new();
    for gate in names
        .into_iter()
        .filter(|gate| name == "all" || gate == name)
    {
        match run_gate(root, &directory.join(format!("{gate}.ron")), static_only) {
            Ok(()) => println!(
                "{gate}: {}passed",
                if static_only { "static checks " } else { "" }
            ),
            Err(error) => failures.push(format!("{gate}: {error}")),
        }
    }
    ensure(failures.is_empty(), failures.join("\n"))
}

fn run_gate(root: &Path, policy: &Path, static_only: bool) -> Result {
    for check in load_ron::<Vec<Check>>(policy)? {
        match check {
            Check::File(path) => ensure(
                root.join(&path).is_file(),
                format!("missing file {}", path.display()),
            )?,
            Check::Directory(path) => ensure(
                root.join(&path).is_dir(),
                format!("missing directory {}", path.display()),
            )?,
            Check::Match {
                pattern,
                paths,
                forbidden,
                literal,
            } => {
                match_paths(root, &pattern, &paths, forbidden, literal)?;
            }
            Check::Cargo {
                args,
                quiet,
                status,
                require,
                forbid,
            } if !static_only => {
                run_cargo(cargo(root).args(args), quiet, status, &require, &forbid)?;
            }
            Check::FileCount {
                directory,
                extension,
                minimum,
                maximum,
            } => {
                let files =
                    fs::read_dir(root.join(&directory))?.collect::<std::io::Result<Vec<_>>>()?;
                let count = files
                    .iter()
                    .filter(|f| {
                        f.path().is_file()
                            && f.path()
                                .extension()
                                .is_some_and(|ext| ext == extension.as_str())
                    })
                    .count();
                ensure(
                    count >= minimum && maximum.is_none_or(|max| count <= max),
                    format!(
                        "{} has {count} .{extension} files, expected minimum {minimum}, maximum {maximum:?}",
                        directory.display()
                    ),
                )?;
            }
            Check::LineLimit { path, maximum } => {
                let lines = fs::read_to_string(root.join(&path))?.lines().count();
                ensure(
                    lines <= maximum,
                    format!("{} has {lines} lines, limit {maximum}", path.display()),
                )?;
            }
            Check::PinnedActions => pinned_actions(root)?,
            Check::TestkitDependents if !static_only => testkit_dependents(root)?,
            Check::SupplyChain if !static_only => supply_chain::check(root)?,
            Check::DatabaseProfiles if !static_only => database_profiles(root)?,
            Check::Cargo { .. }
            | Check::TestkitDependents
            | Check::SupplyChain
            | Check::DatabaseProfiles => {}
        }
    }
    Ok(())
}

fn files(path: &Path, visit: &mut impl FnMut(&Path) -> Result) -> Result {
    if path.is_file() {
        return visit(path);
    }
    let mut entries = fs::read_dir(path)
        .map_err(|error| failure(format!("{}: {error}", path.display())))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let kind = entry.file_type()?;
        if kind.is_file() {
            visit(&entry.path())?;
        } else if kind.is_dir() && !entry.file_name().to_string_lossy().starts_with('.') {
            files(&entry.path(), visit)?;
        }
    }
    Ok(())
}

fn match_paths(
    root: &Path,
    pattern: &str,
    paths: &[PathBuf],
    forbidden: bool,
    literal: bool,
) -> Result {
    let expression = if literal {
        regex::escape(pattern)
    } else {
        pattern.to_owned()
    };
    let matcher = Regex::new(&expression)?;
    let mut seen = false;
    for path in paths {
        files(&root.join(path), &mut |file| {
            let bytes = fs::read(file)?;
            if bytes.contains(&0) {
                return Ok(());
            }
            for (index, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
                if matcher.is_match(line) {
                    ensure(
                        !forbidden,
                        format!(
                            "forbidden pattern {pattern:?} in {}:{}",
                            file.strip_prefix(root).unwrap_or(file).display(),
                            index + 1
                        ),
                    )?;
                    seen = true;
                }
            }
            Ok(())
        })?;
    }
    ensure(
        forbidden || seen,
        format!("pattern {pattern:?} missing in {paths:?}"),
    )
}

fn pinned_actions(root: &Path) -> Result {
    let action = Regex::new(r"uses:\s*([^[:space:]#]+)@([^[:space:]#]+)")?;
    let revision = Regex::new(r"^[0-9a-f]{40}$")?;
    files(&root.join(".github/workflows"), &mut |path| {
        for line in fs::read_to_string(path)?.lines() {
            if let Some(capture) = action.captures(line) {
                ensure(
                    revision.is_match(&capture[2]),
                    format!("mutable GitHub Action reference in {}", path.display()),
                )?;
            }
        }
        Ok(())
    })
}

fn matches(pattern: &str, text: &str) -> Result<bool> {
    let matcher = Regex::new(pattern)?;
    Ok(text.lines().any(|line| matcher.is_match(line)))
}

fn run_cargo(
    command: &mut Command,
    quiet: bool,
    status: i32,
    require: &[String],
    forbid: &[String],
) -> Result {
    if !quiet && status == 0 && require.is_empty() && forbid.is_empty() {
        let actual = command.status()?;
        return ensure(
            actual.success(),
            format!("{command:?} failed with {actual}"),
        );
    }
    let result = output(command)?;
    ensure(
        result.status.code() == Some(status),
        format!("{command:?} returned {}, expected {status}", result.status),
    )?;
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    let text = if status == 0 {
        stdout.into_owned()
    } else {
        format!("{stdout}\n{stderr}")
    };
    for pattern in require {
        ensure(
            matches(pattern, &text)?,
            format!("{command:?} omitted required output pattern {pattern:?}"),
        )?;
    }
    for pattern in forbid {
        ensure(
            !matches(pattern, &text)?,
            format!("{command:?} returned forbidden output pattern {pattern:?}"),
        )?;
    }
    Ok(())
}

fn package_names(bytes: &[u8]) -> BTreeSet<&str> {
    std::str::from_utf8(bytes)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect()
}

fn testkit_dependents(root: &Path) -> Result {
    let out = successful(cargo(root).args([
        "tree",
        "-e",
        "normal",
        "-i",
        "aequora-testkit",
        "--prefix",
        "none",
        "--locked",
        "--offline",
    ]))?;
    let actual = package_names(&out.stdout);
    ensure(
        actual == BTreeSet::from(["aequora-cli", "aequora-testkit"]),
        format!("unexpected testkit dependents {actual:?}"),
    )
}

fn database_profiles(root: &Path) -> Result {
    let profiles: Vec<DatabaseProfile> =
        load_ron(&root.join("architecture/database-profiles.ron"))?;
    for profile in profiles {
        let mut options = vec!["-p", "aequora", "--no-default-features", "--locked"];
        let features = profile.features.join(",");
        if !features.is_empty() {
            options.extend(["--features", &features]);
        }
        run_cargo(
            cargo(root).args(["check", "--lib"]).args(&options),
            false,
            0,
            &[],
            &[],
        )?;
        let result = successful(
            cargo(root)
                .args(["tree", "--edges", "normal", "--prefix", "none"])
                .args(&options),
        )?;
        let packages = package_names(&result.stdout);
        for name in &profile.require {
            ensure(
                packages.contains(name.as_str()),
                format!("{} must include {name}", profile.name),
            )?;
        }
        for name in &profile.forbid {
            ensure(
                !packages.contains(name.as_str()),
                format!("{} unexpectedly includes {name}", profile.name),
            )?;
        }
        println!("database-neutrality policy: {} compiles", profile.name);
    }
    Ok(())
}
