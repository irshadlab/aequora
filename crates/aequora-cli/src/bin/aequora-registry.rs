use aequora_registry::codegen::{
    artifact, diff, generated_markdown, generated_rust, load_lock, load_registry, make_lock,
    verify_lock,
};
use aequora_registry::types::{AllocationClass, RegistryDomain};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "aequora-registry",
    about = "Aequora protocol registry maintenance"
)]
struct RegistryCli {
    #[command(subcommand)]
    command: Option<RegistryCommand>,
}

#[derive(Subcommand)]
enum RegistryCommand {
    /// Verify the registry and generated artifacts.
    #[command(visible_alias = "lint")]
    Verify {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// Generate or verify registry documentation.
    Docs {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        check: bool,
    },
    /// Generate or verify the registry Rust snapshot.
    Rust {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        check: bool,
    },
    /// Generate the registry lockfile.
    Lock {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// Explain one registry entry.
    Explain {
        root: PathBuf,
        #[arg(value_parser = parse_domain)]
        domain: RegistryDomain,
        id: u32,
    },
    /// Find the next available ID in an allocation range.
    Reserve {
        root: PathBuf,
        #[arg(value_parser = parse_domain)]
        domain: RegistryDomain,
        #[arg(value_enum, default_value = "core")]
        allocation: Allocation,
    },
    /// Compare two registries.
    Diff { old: PathBuf, new: PathBuf },
    /// Compare two registries and enforce compatibility.
    Compatibility { old: PathBuf, new: PathBuf },
    /// Write shell completions to stdout.
    Completions {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Write the manual page to stdout.
    Man,
}

#[derive(Clone, Copy, ValueEnum)]
enum Allocation {
    Core,
    Extension,
    Private,
    Experimental,
}

impl From<Allocation> for AllocationClass {
    fn from(value: Allocation) -> Self {
        match value {
            Allocation::Core => Self::Core,
            Allocation::Extension => Self::RegisteredExtension,
            Allocation::Private => Self::VendorPrivate,
            Allocation::Experimental => Self::Experimental,
        }
    }
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<(), Box<dyn Error>> {
    let Some(command) = RegistryCli::parse().command else {
        RegistryCli::command().print_help()?;
        println!();
        return Ok(());
    };
    let compatibility = matches!(command, RegistryCommand::Compatibility { .. });
    match command {
        RegistryCommand::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut RegistryCli::command(),
                "aequora-registry",
                &mut io::stdout().lock(),
            );
        }
        RegistryCommand::Man => {
            clap_mangen::Man::new(RegistryCli::command()).render(&mut io::stdout().lock())?;
        }
        RegistryCommand::Verify { root } => verify(&root)?,
        RegistryCommand::Docs { root, check } => {
            let set = load_registry(&root)?;
            artifact(
                &root.join("docs/registry/generated-registry.md"),
                &generated_markdown(&set),
                check,
            )?;
            println!(
                "registry docs {}",
                if check { "verified" } else { "generated" }
            );
        }
        RegistryCommand::Rust { root, check } => {
            let set = load_registry(&root)?;
            artifact(
                &root.join("crates/aequora-registry/src/registry.rs"),
                &generated_rust(&set),
                check,
            )?;
            println!(
                "registry Rust snapshot {}",
                if check { "verified" } else { "generated" }
            );
        }
        RegistryCommand::Lock { root } => {
            let set = load_registry(&root)?;
            let content =
                ron::ser::to_string_pretty(&make_lock(&set), ron::ser::PrettyConfig::default())?;
            fs::write(root.join("registry/registry.lock"), format!("{content}\n"))?;
            println!("registry lock generated");
        }
        RegistryCommand::Explain { root, domain, id } => {
            let set = load_registry(&root)?;
            let entry = set
                .entries
                .iter()
                .find(|entry| entry.domain == domain && entry.id == id)
                .ok_or("unknown registry ID")?;
            println!(
                "{}:{} {} [{:?}]\nowner={} crate={} module={}\n{}",
                domain,
                id,
                entry.name,
                entry.status,
                entry.owner.team,
                entry.owner.crate_name,
                entry.owner.module,
                entry.description
            );
            for (key, value) in &entry.attributes {
                println!("{key}={value}");
            }
        }
        RegistryCommand::Reserve {
            root,
            domain,
            allocation,
        } => {
            let set = load_registry(&root)?;
            println!(
                "{}",
                next_id(
                    &set.entries
                        .iter()
                        .filter(|entry| entry.domain == domain)
                        .map(|entry| entry.id)
                        .collect::<Vec<_>>(),
                    allocation.into()
                )
                .ok_or("ID range exhausted")?
            );
        }
        RegistryCommand::Diff { old, new } | RegistryCommand::Compatibility { old, new } => {
            let changes = diff(&load_registry(&old)?, &load_registry(&new)?);
            for change in &changes {
                println!(
                    "{:?} {}:{} {}",
                    change.classification, change.domain, change.id, change.summary
                );
            }
            if compatibility
                && changes.iter().any(|change| {
                    matches!(
                        change.classification,
                        aequora_registry::types::ChangeClassification::BreakingWithMigration
                            | aequora_registry::types::ChangeClassification::SecurityRequired
                    )
                })
            {
                return Err("registry contains changes requiring explicit approval and migration/security review".into());
            }
        }
    }
    Ok(())
}

fn verify(root: &Path) -> Result<(), Box<dyn Error>> {
    let set = load_registry(root)?;
    let lock = load_lock(root)?;
    verify_lock(&set, &lock)?;
    artifact(
        &root.join("docs/registry/generated-registry.md"),
        &generated_markdown(&set),
        true,
    )?;
    println!(
        "registry verified generation={} entries={}",
        set.manifest.generation,
        set.entries.len()
    );
    Ok(())
}

fn parse_domain(value: &str) -> Result<RegistryDomain, String> {
    match value {
        "entity" => Ok(RegistryDomain::Entity),
        "operation" => Ok(RegistryDomain::Operation),
        "event" => Ok(RegistryDomain::Event),
        "field" => Ok(RegistryDomain::Field),
        "capability" => Ok(RegistryDomain::Capability),
        "profile" => Ok(RegistryDomain::ConsistencyProfile),
        "error" => Ok(RegistryDomain::Error),
        "job" => Ok(RegistryDomain::Job),
        "consumer" => Ok(RegistryDomain::Consumer),
        "audit-action" => Ok(RegistryDomain::AuditAction),
        "migration" => Ok(RegistryDomain::Migration),
        "protocol" => Ok(RegistryDomain::Protocol),
        "message" => Ok(RegistryDomain::Message),
        "permission" => Ok(RegistryDomain::Permission),
        "admin-action" => Ok(RegistryDomain::AdminAction),
        "reason" => Ok(RegistryDomain::Reason),
        "decision-rule" => Ok(RegistryDomain::DecisionRule),
        "artifact-format" => Ok(RegistryDomain::ArtifactFormat),
        "conformance-profile" => Ok(RegistryDomain::ConformanceProfile),
        "conformance-test" => Ok(RegistryDomain::ConformanceTest),
        "certification-tier" => Ok(RegistryDomain::CertificationTier),
        _ => Err(format!("unknown registry domain `{value}`")),
    }
}

fn next_id(used: &[u32], allocation: AllocationClass) -> Option<u32> {
    let (start, end) = match allocation {
        AllocationClass::Core => (1, 0x3fff_ffff),
        AllocationClass::RegisteredExtension => (0x4000_0000, 0xbfff_ffff),
        AllocationClass::VendorPrivate => (0xc000_0000, 0xefff_ffff),
        AllocationClass::Experimental => (0xf000_0000, u32::MAX),
    };
    (start..=end).find(|candidate| !used.contains(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservation_stays_inside_allocation_range() {
        assert_eq!(next_id(&[1, 2], AllocationClass::Core), Some(3));
        assert_eq!(
            next_id(&[], AllocationClass::Experimental),
            Some(0xf000_0000)
        );
    }
}
