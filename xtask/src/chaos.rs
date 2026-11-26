use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::Duration,
};

use serde::Deserialize;

use crate::{Result, ensure, failure, load_ron};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    manifest: PathBuf,
    container: String,
    executable: PathBuf,
    endpoint: String,
    readiness_attempts: usize,
    readiness_delay_ms: u64,
    scenarios: Vec<Scenario>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    name: String,
    clients: usize,
    duration: usize,
    batch_size: usize,
    #[serde(default)]
    hot_keys: usize,
    #[serde(default)]
    adversarial_clients: usize,
    #[serde(default)]
    disruption: Option<Disruption>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
enum Disruption {
    Pause { after_ms: u64, duration_ms: u64 },
    Restart { after_ms: u64 },
}

struct Harness {
    manifest: tempfile::NamedTempFile,
    child: Option<Child>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = Command::new("podman")
            .args(["play", "kube", "--down"])
            .arg(self.manifest.path())
            .output();
    }
}

fn run_command(command: &mut Command, description: &str) -> Result {
    let status = command.status()?;
    ensure(
        status.success(),
        format!("{description} failed with {status}"),
    )
}

pub(super) fn run(root: &Path) -> Result {
    let policy: Policy = load_ron(&root.join("architecture/chaos.ron"))?;
    let executable = root.join(&policy.executable);
    ensure(
        executable.is_file(),
        format!("missing stress executable {}", executable.display()),
    )?;
    ensure(
        policy.endpoint == "http://127.0.0.1:8443",
        "stress harness endpoint must be local",
    )?;
    for command in ["podman", "curl"] {
        ensure(
            Command::new(command)
                .arg("--version")
                .output()?
                .status
                .success(),
            format!("{command} is unavailable"),
        )?;
    }
    let mut secret = [0_u8; 32];
    getrandom::fill(&mut secret).map_err(|error| {
        failure(format!(
            "cannot generate a stress authentication key: {error}"
        ))
    })?;
    let secret = hex::encode(secret);
    let template = fs::read_to_string(root.join(&policy.manifest))?;
    ensure(
        template.contains("AEQUORA_STRESS_AUTH_KEY_HEX_VALUE"),
        "stress manifest is missing its key placeholder",
    )?;
    let mut manifest = tempfile::Builder::new()
        .prefix("aequora-stress-manifest.")
        .suffix(".yaml")
        .tempfile()?;
    manifest.write_all(
        template
            .replace("AEQUORA_STRESS_AUTH_KEY_HEX_VALUE", &secret)
            .as_bytes(),
    )?;
    let mut harness = Harness {
        manifest,
        child: None,
    };

    run_command(
        Command::new("podman")
            .args(["play", "kube"])
            .arg(harness.manifest.path()),
        "deploying the stress pod",
    )?;
    wait_for_readiness(&policy)?;
    for scenario in &policy.scenarios {
        run_scenario(root, &policy, scenario, &executable, &secret, &mut harness)?;
    }
    Ok(())
}

fn wait_for_readiness(policy: &Policy) -> Result {
    let readiness_url = format!("{}/sync/v1/health/ready", policy.endpoint);
    for _ in 0..policy.readiness_attempts {
        let result = Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--max-time",
                "1",
                "--output",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
                "--write-out",
                "%{http_code}",
                "--proto",
                "=http",
                &readiness_url,
            ])
            .output()?;
        if result.status.success() && result.stdout == b"204" {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(policy.readiness_delay_ms));
    }
    let _ = Command::new("podman")
        .args(["logs", &policy.container])
        .status();
    Err(failure("stress server did not become ready"))
}

fn run_scenario(
    root: &Path,
    policy: &Policy,
    scenario: &Scenario,
    executable: &Path,
    secret: &str,
    harness: &mut Harness,
) -> Result {
    println!("Running {}", scenario.name);
    let mut client = Command::new(executable);
    client
        .current_dir(root)
        .env("AEQUORA_STRESS_AUTH_KEY_HEX", secret)
        .args([
            "--url",
            &policy.endpoint,
            "--concurrency",
            &scenario.clients.to_string(),
            "--duration",
            &scenario.duration.to_string(),
            "--batch-size",
            &scenario.batch_size.to_string(),
            "--skip-functional",
        ]);
    if scenario.hot_keys != 0 {
        client.args(["--hot-keys", &scenario.hot_keys.to_string()]);
    }
    if scenario.adversarial_clients != 0 {
        client.args([
            "--adversarial-clients",
            &scenario.adversarial_clients.to_string(),
        ]);
    }
    if let Some(disruption) = scenario.disruption {
        harness.child = Some(client.spawn()?);
        let podman = |command: &str| {
            run_command(
                Command::new("podman").args([command, policy.container.as_str()]),
                command,
            )
        };
        match disruption {
            Disruption::Pause {
                after_ms,
                duration_ms,
            } => {
                thread::sleep(Duration::from_millis(after_ms));
                podman("pause")?;
                run_command(
                    Command::new("podman").args([
                        "ps",
                        "--filter",
                        &format!("name={}", policy.container),
                    ]),
                    "listing the paused container",
                )?;
                thread::sleep(Duration::from_millis(duration_ms));
                podman("unpause")?;
            }
            Disruption::Restart { after_ms } => {
                thread::sleep(Duration::from_millis(after_ms));
                podman("restart")?;
            }
        }
        let child = harness
            .child
            .as_mut()
            .ok_or_else(|| failure("stress client is missing"))?;
        let status = child.wait()?;
        harness.child = None;
        ensure(
            status.success(),
            format!("stress client failed with {status}"),
        )?;
    } else {
        run_command(&mut client, "stress client")?;
    }
    run_command(
        Command::new("podman").args(["stats", "--no-stream", &policy.container]),
        "pod resource statistics",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_policy_keeps_all_three_local_scenarios() -> Result {
        let policy: Policy = ron::from_str(include_str!("../../architecture/chaos.ron"))?;
        assert_eq!(policy.endpoint, "http://127.0.0.1:8443");
        assert!(matches!(
            policy.scenarios.as_slice(),
            [
                Scenario {
                    disruption: None,
                    ..
                },
                Scenario {
                    disruption: Some(Disruption::Pause { .. }),
                    ..
                },
                Scenario {
                    disruption: Some(Disruption::Restart { .. }),
                    ..
                }
            ]
        ));
        Ok(())
    }
}
