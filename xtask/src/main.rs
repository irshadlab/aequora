mod chaos;
mod check;
mod publish;
mod supply_chain;

use std::{
    env,
    error::Error,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Output},
};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    match run(&root) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(root: &Path) -> Result {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("check") => check::run(root, &args.collect::<Vec<_>>()),
        Some("publish") => publish::run(root, args),
        Some("supply-chain") => {
            let output = args
                .next()
                .map_or_else(|| root.join("target/supply-chain"), PathBuf::from);
            let artifact = args
                .next()
                .map_or_else(|| root.join("Cargo.lock"), PathBuf::from);
            ensure(
                args.next().is_none(),
                "usage: cargo xtask supply-chain [output] [artifact]",
            )?;
            supply_chain::generate(root, &output, &artifact)?;
            println!("supply-chain artifacts: {}", output.display());
            Ok(())
        }
        Some("rag") => rag(root, &args.collect::<Vec<_>>()),
        Some("chaos") => {
            ensure(args.next().is_none(), "usage: cargo xtask chaos")?;
            chaos::run(root)
        }
        None | Some("help" | "--help" | "-h") => {
            println!("cargo xtask check <name|all|list> [--static]");
            println!("cargo xtask publish --plan [--crate NAME] [--layer NUMBER]");
            println!("cargo xtask supply-chain [output] [artifact]");
            println!("cargo xtask rag <index|query|code|docs|stats|serve> [args]");
            println!("cargo xtask chaos");
            Ok(())
        }
        Some(name) => Err(failure(format!("unknown command {name}"))),
    }
}

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::other(message.into()).into()
}

fn ensure(condition: bool, message: impl Into<String>) -> Result {
    if condition {
        Ok(())
    } else {
        Err(failure(message))
    }
}

fn load_ron<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    ron::from_str(&fs::read_to_string(path)?)
        .map_err(|error| failure(format!("{}: {error}", path.display())))
}

fn cargo(root: &Path) -> Command {
    let executable = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(executable);
    command.current_dir(root);
    if env::var_os("CARGO_BUILD_JOBS").is_none() {
        command.env("CARGO_BUILD_JOBS", "1");
    }
    command
}

fn output(command: &mut Command) -> Result<Output> {
    command
        .output()
        .map_err(|error| failure(format!("{command:?}: {error}")))
}

fn successful(command: &mut Command) -> Result<Output> {
    let result = output(command)?;
    ensure(
        result.status.success(),
        format!(
            "{command:?} failed with {}\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        ),
    )?;
    Ok(result)
}

fn rag(root: &Path, args: &[String]) -> Result {
    let mut command = Command::new("octocode");
    command.current_dir(root);
    let Some((verb, rest)) = args.split_first() else {
        return Err(failure(
            "usage: cargo xtask rag <index|query|code|docs|stats|serve> [args]",
        ));
    };
    match verb.as_str() {
        "index" => {
            command.args(["index", "--no-git"]);
        }
        "query" | "code" | "docs" => {
            ensure(!rest.is_empty(), "rag search requires a query")?;
            command.args([
                "search",
                "--threshold",
                "0.25",
                "--format",
                "text",
                "--detail-level",
                "partial",
            ]);
            if verb != "query" {
                command.args(["--mode", verb]);
            }
        }
        "stats" => {
            command.arg("stats");
        }
        "serve" => {
            command.args(["mcp", "--path", ".", "--with-lsp", "rust-analyzer"]);
        }
        _ => return Err(failure(format!("unknown RAG command {verb}"))),
    }
    command.args(rest);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        Err(command.exec().into())
    }
    #[cfg(not(unix))]
    {
        let status = command.status()?;
        ensure(status.success(), format!("octocode exited with {status}"))
    }
}
