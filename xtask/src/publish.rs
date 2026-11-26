use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::Command,
    thread,
    time::Duration,
};

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::{Result, cargo, ensure, failure, load_ron, output, successful};

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: BTreeSet<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    publish: Option<Vec<String>>,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    crates: BTreeSet<String>,
    delay_seconds: u64,
    rate_limit_attempts: usize,
    rate_limit_wait_seconds: u64,
    propagation_retries: usize,
}

struct Plan {
    packages: BTreeMap<String, Package>,
    layers: Vec<Vec<String>>,
    policy: Policy,
}

impl Plan {
    fn load(root: &Path) -> Result<Self> {
        let policy: Policy = load_ron(&root.join("release/publish.ron"))?;
        ensure(!policy.crates.is_empty(), "publish allowlist is empty")?;
        let output = successful(cargo(root).args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--locked",
        ]))?;
        let metadata: Metadata = serde_json::from_slice(&output.stdout)?;
        let packages = metadata
            .packages
            .into_iter()
            .filter(|pkg| metadata.workspace_members.contains(&pkg.id))
            .map(|pkg| (pkg.name.clone(), pkg))
            .collect::<BTreeMap<_, _>>();
        let mut remaining = BTreeMap::new();
        for name in &policy.crates {
            let package = packages
                .get(name)
                .ok_or_else(|| failure(format!("unknown publishable package {name}")))?;
            ensure(
                package
                    .publish
                    .as_ref()
                    .is_none_or(|registries| !registries.is_empty()),
                format!("{name} has publish = false"),
            )?;
            let dependencies = package
                .dependencies
                .iter()
                .filter(|dep| dep.path.is_some() && policy.crates.contains(&dep.name))
                .map(|dep| dep.name.clone())
                .collect::<BTreeSet<_>>();
            remaining.insert(name.clone(), dependencies);
        }
        let mut layers = Vec::new();
        while !remaining.is_empty() {
            let ready = remaining
                .iter()
                .filter(|(_, deps)| deps.is_empty())
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            ensure(
                !ready.is_empty(),
                format!(
                    "publish cycle among {:?}",
                    remaining.keys().collect::<Vec<_>>()
                ),
            )?;
            for name in &ready {
                remaining.remove(name);
            }
            for dependencies in remaining.values_mut() {
                for name in &ready {
                    dependencies.remove(name);
                }
            }
            layers.push(ready);
        }
        Ok(Self {
            packages,
            layers,
            policy,
        })
    }

    fn print(&self) {
        println!("Total workspace crates:   {}", self.packages.len());
        println!("Publishable crates:       {}", self.policy.crates.len());
        println!("Topological layers:       {}", self.layers.len());
        for (index, layer) in self.layers.iter().enumerate() {
            println!(
                "\n--- Layer {}/{} ({} crates) ---\n{}",
                index + 1,
                self.layers.len(),
                layer.len(),
                layer.join(", ")
            );
        }
    }
}

pub(super) fn plan(root: &Path) -> Result {
    Plan::load(root).map(|_| ())
}

#[derive(Default)]
struct Options {
    plan: bool,
    dry_run: bool,
    cargo_flags: Vec<String>,
    skip_api_check: bool,
    crate_name: Option<String>,
    layer: Option<usize>,
    delay: Option<u64>,
    propagation_retries: Option<usize>,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut options = Self::default();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--plan" => options.plan = true,
                "--dry-run" => options.dry_run = true,
                "--allow-dirty" | "--no-verify" => options.cargo_flags.push(arg),
                "--skip-api-check" => options.skip_api_check = true,
                "--crate" | "--layer" | "--delay" | "--propagation-retries" => {
                    let value = args
                        .next()
                        .ok_or_else(|| failure(format!("{arg} needs a value")))?;
                    match arg.as_str() {
                        "--crate" => options.crate_name = Some(value),
                        "--layer" => options.layer = Some(value.parse()?),
                        "--delay" => options.delay = Some(value.parse()?),
                        _ => options.propagation_retries = Some(value.parse()?),
                    }
                }
                _ => return Err(failure(format!("unknown publish option {arg}"))),
            }
        }
        Ok(options)
    }
}

pub(super) fn run(root: &Path, args: impl Iterator<Item = String>) -> Result {
    let mut args = args.peekable();
    if args
        .peek()
        .is_some_and(|flag| flag == "--help" || flag == "-h")
    {
        println!(
            "cargo xtask publish [--plan|--dry-run] [--crate NAME] [--layer NUMBER] [--delay SECONDS] [--propagation-retries N] [--allow-dirty] [--no-verify] [--skip-api-check]"
        );
        return Ok(());
    }
    let options = Options::parse(args)?;
    let plan = Plan::load(root)?;
    validate_selection(&plan.layers, options.crate_name.as_deref(), options.layer)?;
    plan.print();
    if options.plan {
        return Ok(());
    }
    for (index, layer) in plan.layers.iter().enumerate() {
        if options.layer.is_some_and(|layer| layer != index + 1) {
            continue;
        }
        for name in layer {
            if options
                .crate_name
                .as_ref()
                .is_some_and(|target| target != name)
            {
                continue;
            }
            publish(root, &plan, &options, &plan.packages[name])?;
        }
    }
    Ok(())
}

fn validate_selection(layers: &[Vec<String>], name: Option<&str>, layer: Option<usize>) -> Result {
    let selected = if let Some(layer) = layer {
        let index = layer
            .checked_sub(1)
            .ok_or_else(|| failure("publish layers start at 1"))?;
        layers
            .get(index..=index)
            .ok_or_else(|| failure(format!("unknown publish layer {layer}")))?
    } else {
        layers
    };
    if let Some(name) = name {
        ensure(
            selected.iter().flatten().any(|package| package == name),
            match layer {
                Some(layer) => format!("{name} is not publishable in layer {layer}"),
                None => format!("{name} is not publishable"),
            },
        )?;
    }
    Ok(())
}

fn already_published(package: &Package) -> bool {
    let url = format!(
        "https://crates.io/api/v1/crates/{}/{}",
        package.name, package.version
    );
    let Ok(result) = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--max-time",
            "10",
            "--proto",
            "=https",
            "--user-agent",
            "aequora-publish-orchestrator (github.com/irshadali5/aequora)",
            &url,
        ])
        .output()
    else {
        return false;
    };
    result.status.success()
        && serde_json::from_slice::<Value>(&result.stdout)
            .is_ok_and(|body| body["version"]["num"] == package.version)
}

fn publish(root: &Path, plan: &Plan, options: &Options, package: &Package) -> Result {
    let name = &package.name;
    if !options.skip_api_check && already_published(package) {
        println!("{name} {} already published", package.version);
        return Ok(());
    }
    let mut command = cargo(root);
    command.args(["publish", "-p", name]);
    command.args(&options.cargo_flags);
    if options.dry_run {
        command.arg("--dry-run");
    }
    let delay = options.delay.unwrap_or(plan.policy.delay_seconds);
    let propagation_retries = options
        .propagation_retries
        .unwrap_or(plan.policy.propagation_retries);
    let missing = Regex::new(r"no matching package named [`']([^`']+)[`'] found")?;
    let mut rate_limit_attempt = 0;
    let mut propagation_attempt = 0;
    loop {
        println!(
            "Publishing {name}{}",
            if options.dry_run { " (dry run)" } else { "" }
        );
        let result = output(&mut command)?;
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);
        if result.status.success() || (!options.dry_run && stderr.contains("already exists")) {
            print!("{stdout}");
            if delay > 0 && !options.dry_run && options.crate_name.is_none() {
                thread::sleep(Duration::from_secs(delay));
            }
            return Ok(());
        }
        let lower = stderr.to_lowercase();
        if !options.dry_run
            && options.crate_name.is_none()
            && (lower.contains("429")
                || lower.contains("rate limit")
                || lower.contains("too many requests"))
        {
            rate_limit_attempt += 1;
            ensure(
                rate_limit_attempt < plan.policy.rate_limit_attempts,
                format!("publish rate limit for {name}: {stderr}"),
            )?;
            println!("Waiting for the crates.io rate limit to reset");
            thread::sleep(Duration::from_secs(plan.policy.rate_limit_wait_seconds));
        } else if let Some(dependency) = missing
            .captures(&stderr)
            .map(|captures| captures[1].to_owned())
            .filter(|dependency| {
                !options.dry_run
                    && options.crate_name.is_none()
                    && plan.packages.contains_key(dependency)
            })
        {
            propagation_attempt += 1;
            ensure(
                propagation_attempt <= propagation_retries,
                format!("{dependency} did not propagate: {stderr}"),
            )?;
            println!("Waiting for {dependency} to appear in the index");
            thread::sleep(Duration::from_secs(delay.max(plan.policy.delay_seconds)));
        } else {
            return Err(failure(format!("publish failed for {name}: {stderr}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate_selection;

    #[test]
    fn conflicting_publish_selectors_are_rejected() {
        let layers = [vec!["types".into()], vec!["protocol".into()]];
        assert!(validate_selection(&layers, Some("types"), Some(1)).is_ok());
        assert!(validate_selection(&layers, Some("types"), Some(2)).is_err());
        assert!(validate_selection(&layers, Some("missing"), None).is_err());
        assert!(validate_selection(&layers, None, Some(0)).is_err());
        assert!(validate_selection(&layers, None, Some(3)).is_err());
    }
}
