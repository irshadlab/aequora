use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs::{self, File},
    io::Read as _,
    path::Path,
    process::Command,
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use spdx_expression::SpdxExpression;
use spdx_rs::models::{
    Algorithm, Checksum, CreationInfo, DocumentCreationInformation, ExternalPackageReference,
    ExternalPackageReferenceCategory, PackageInformation, Relationship, RelationshipType, SPDX,
};

use crate::{Result, cargo, ensure, failure, successful};

fn metadata(root: &Path) -> Result<Value> {
    let output = successful(cargo(root).args([
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--offline",
    ]))?;
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn array<'a>(document: &'a Value, key: &str) -> Result<&'a [Value]> {
    document[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| failure(format!("Cargo metadata is missing {key}")))
}

fn sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

pub(super) fn generate(root: &Path, output: &Path, artifact: &Path) -> Result {
    generate_from(root, output, artifact, &metadata(root)?)
}

fn generate_from(root: &Path, output: &Path, artifact: &Path, graph: &Value) -> Result {
    let artifact_hash = sha256(artifact)?;
    let lock_hash = sha256(&root.join("Cargo.lock"))?;
    let source = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output();
    let commit = source
        .ok()
        .filter(|result| result.status.success())
        .map_or_else(
            || "0".repeat(40),
            |result| String::from_utf8_lossy(&result.stdout).trim().to_owned(),
        );
    let artifact_name = artifact
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| failure("the artifact needs a UTF-8 filename"))?;
    let packages = array(graph, "packages")?;
    let spdx = spdx_document(graph, artifact_name, &commit, &artifact_hash, &lock_hash)?;
    let (licenses, notices, report) = inventory(packages, &commit)?;
    fs::create_dir_all(output)?;
    fs::write(
        output.join("aequora.spdx.json"),
        format!("{}\n", serde_json::to_string_pretty(&spdx)?),
    )?;
    fs::write(
        output.join("dependency-report.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    fs::write(output.join("THIRD_PARTY_LICENSES.txt"), licenses)?;
    fs::write(output.join("THIRD_PARTY_NOTICES.md"), notices)?;
    Ok(())
}

fn spdx_document(
    graph: &Value,
    artifact_name: &str,
    commit: &str,
    artifact_hash: &str,
    lock_hash: &str,
) -> Result<SPDX> {
    const ARTIFACT_ID: &str = "SPDXRef-AequoraArtifact";
    let mut packages = vec![PackageInformation {
        package_name: artifact_name.to_owned(),
        package_spdx_identifier: ARTIFACT_ID.to_owned(),
        package_version: Some(commit.to_owned()),
        files_analyzed: Some(false),
        concluded_license: Some(SpdxExpression::parse("NOASSERTION")?),
        declared_license: Some(SpdxExpression::parse("MIT")?),
        package_checksum: vec![Checksum::new(Algorithm::SHA256, artifact_hash)],
        external_reference: vec![ExternalPackageReference::new(
            ExternalPackageReferenceCategory::Other,
            "aequora-lockfile-sha256".to_owned(),
            lock_hash.to_owned(),
            None,
        )],
        ..PackageInformation::default()
    }];
    let mut references = BTreeMap::new();
    for package in array(graph, "packages")? {
        let field = |key: &str| {
            package[key]
                .as_str()
                .ok_or_else(|| failure(format!("Cargo package is missing {key}")))
        };
        let (id, name, version) = (field("id")?, field("name")?, field("version")?);
        let source = package["source"].as_str().unwrap_or("NOASSERTION");
        // Cargo IDs for path dependencies contain absolute checkout paths.
        let identity = format!("{name}\n{version}\n{source}");
        let digest = hex::encode(Sha256::digest(identity.as_bytes()));
        let reference = format!("SPDXRef-Package-{}-{digest}", name.replace('_', "-"));
        references.insert(id, reference.clone());
        let license = SpdxExpression::parse(
            &package["license"]
                .as_str()
                .unwrap_or("NOASSERTION")
                .replace('/', " OR "),
        )?;
        packages.push(PackageInformation {
            package_name: name.to_owned(),
            package_spdx_identifier: reference,
            package_version: Some(version.to_owned()),
            package_download_location: source.to_owned(),
            files_analyzed: Some(false),
            concluded_license: Some(SpdxExpression::parse("NOASSERTION")?),
            declared_license: Some(license),
            package_supplier: Some("NOASSERTION".to_owned()),
            ..PackageInformation::default()
        });
    }
    packages[1..]
        .sort_unstable_by(|a, b| a.package_spdx_identifier.cmp(&b.package_spdx_identifier));
    let reference = |id: &Value| {
        id.as_str()
            .and_then(|id| references.get(id))
            .ok_or_else(|| failure("Cargo dependency references a missing SPDX package"))
    };
    let mut relationships = Vec::new();
    for node in array(&graph["resolve"], "nodes")? {
        for dependency in array(node, "deps")? {
            relationships.push(Relationship::new(
                reference(&node["id"])?,
                reference(&dependency["pkg"])?,
                RelationshipType::DependsOn,
                None,
            ));
        }
    }
    relationships.sort_unstable_by(|a, b| {
        (&a.spdx_element_id, &a.related_spdx_element)
            .cmp(&(&b.spdx_element_id, &b.related_spdx_element))
    });
    relationships.dedup();
    Ok(SPDX {
        document_creation_information: DocumentCreationInformation {
            spdx_version: "SPDX-2.3".to_owned(),
            data_license: "CC0-1.0".to_owned(),
            spdx_identifier: "SPDXRef-DOCUMENT".to_owned(),
            document_name: format!("aequora-{artifact_name}"),
            spdx_document_namespace: format!(
                "https://aequora.invalid/spdx/{commit}/{artifact_hash}"
            ),
            external_document_references: Vec::new(),
            creation_info: CreationInfo {
                creators: vec!["Tool: cargo xtask supply-chain".to_owned()],
                created: "1970-01-01T00:00:00Z".parse()?,
                license_list_version: None,
                creator_comment: None,
            },
            document_comment: None,
            document_describes: vec![ARTIFACT_ID.to_owned()],
        },
        package_information: packages,
        relationships,
        other_licensing_information_detected: Vec::new(),
        file_information: Vec::new(),
        snippet_information: Vec::new(),
        annotations: Vec::new(),
        spdx_ref_counter: 0,
    })
}

fn inventory(packages: &[Value], commit: &str) -> Result<(String, String, Value)> {
    let external = packages
        .iter()
        .filter(|package| !package["source"].is_null())
        .collect::<Vec<_>>();

    let mut rows = Vec::new();
    let mut versions = BTreeMap::<&str, Vec<&Value>>::new();
    for package in &external {
        let name = package["name"].as_str().unwrap_or_default();
        versions.entry(name).or_default().push(&package["version"]);
        rows.push([
            name,
            package["version"].as_str().unwrap_or_default(),
            package["license"].as_str().unwrap_or("UNKNOWN"),
            package["source"].as_str().unwrap_or_default(),
        ]);
    }
    rows.sort_unstable();
    rows.dedup();
    let mut licenses = String::new();
    let mut notices = format!(
        "# Third-party notices\n\nGenerated from the locked Cargo graph for source commit {commit}.\nPackage license metadata is reproduced below for review. Include applicable upstream license and notice files with the distributed artifact.\n\n| Package | Version | Declared license | Source |\n| --- | --- | --- | --- |\n"
    );
    for [name, version, license, source] in rows {
        writeln!(licenses, "{name}\t{version}\t{license}\t{source}")?;
        writeln!(notices, "| {name} | {version} | {license} | {source} |")?;
    }
    let duplicate_versions = versions
        .into_iter()
        .filter(|(_, versions)| versions.len() > 1)
        .map(|(name, versions)| json!({"name": name, "versions": versions}))
        .collect::<Vec<_>>();
    let describe = |package: &Value| json!({"name": package["name"], "version": package["version"], "source": package["source"]});
    let targets = |kind: &str| -> Vec<Value> {
        packages
            .iter()
            .filter(|package| {
                package["targets"].as_array().is_some_and(|targets| {
                    targets.iter().any(|target| {
                        target["kind"]
                            .as_array()
                            .is_some_and(|kinds| kinds.iter().any(|value| value == kind))
                    })
                })
            })
            .map(describe)
            .collect()
    };
    let report = json!({
        "resolved_packages": packages.len(), "external_packages": external.len(),
        "duplicate_versions": duplicate_versions,
        "native_boundaries": packages.iter().filter(|package| !package["links"].is_null()).map(|package|
            json!({"name": package["name"], "version": package["version"], "links": package["links"], "source": package["source"]})).collect::<Vec<_>>(),
        "build_scripts": targets("custom-build"), "proc_macros": targets("proc-macro"),
        "sources": external.iter().map(|package| describe(package)).collect::<Vec<_>>(),
        "licenses": external.iter().map(|package| json!({"name": package["name"], "version": package["version"], "license": package["license"]})).collect::<Vec<_>>()
    });
    Ok((licenses, notices, report))
}

pub(super) fn check(root: &Path) -> Result {
    let graph = metadata(root)?;
    for package in array(&graph, "packages")? {
        if let Some(source) = package["source"].as_str() {
            let name = &package["name"];
            ensure(
                package["license"]
                    .as_str()
                    .is_some_and(|license| !license.is_empty()),
                format!("{name} lacks license metadata"),
            )?;
            ensure(
                source.starts_with("registry+https://github.com/rust-lang/crates.io-index"),
                format!("{name} has an unapproved source"),
            )?;
        }
    }
    crate::publish::plan(root)?;
    for (args, marker) in [
        (
            vec!["deps", "verify", "supply-chain/policy.ron"],
            "dependency policy: valid",
        ),
        (
            vec![
                "supply-chain",
                "verify",
                "supply-chain/policy.ron",
                "tests/fixtures/part49/server.spdx.ron",
                "tests/fixtures/part49/server-release-evidence.ron",
            ],
            "supply-chain: verified",
        ),
    ] {
        let result = successful(
            cargo(root)
                .args([
                    "run",
                    "-q",
                    "-p",
                    "aequora-cli",
                    "--locked",
                    "--offline",
                    "--",
                ])
                .args(args),
        )?;
        ensure(
            String::from_utf8_lossy(&result.stdout).contains(marker),
            format!("missing evidence marker {marker}"),
        )?;
    }
    let directory = tempfile::tempdir()?;
    generate_from(root, directory.path(), &root.join("Cargo.lock"), &graph)?;
    let sbom: SPDX =
        serde_json::from_reader(File::open(directory.path().join("aequora.spdx.json"))?)?;
    ensure(
        sbom.document_creation_information.spdx_version == "SPDX-2.3"
            && sbom.package_information.len() > 1
            && sbom.package_information[0].package_checksum
                == [Checksum::new(
                    Algorithm::SHA256,
                    &sha256(&root.join("Cargo.lock"))?,
                )]
            && !sbom.relationships.is_empty(),
        "invalid SPDX evidence",
    )?;
    for name in ["THIRD_PARTY_LICENSES.txt", "THIRD_PARTY_NOTICES.md"] {
        ensure(
            fs::metadata(directory.path().join(name))?.len() > 0,
            format!("empty {name}"),
        )?;
    }
    let report: Value =
        serde_json::from_reader(File::open(directory.path().join("dependency-report.json"))?)?;
    ensure(
        report["resolved_packages"].as_u64().unwrap_or_default() > 1,
        "dependency report is empty",
    )?;
    for name in ["build_scripts", "proc_macros", "native_boundaries"] {
        array(&report, name)?;
    }
    Ok(())
}
