# RTK - Rust Token Killer (Codex CLI)

Use RTK's token-optimized proxies for noisy shell commands when the corresponding proxy exists.

```bash
rtk git status
rtk rg "OperationEnvelope" crates
rtk cargo check --workspace --all-features
rtk cargo test --workspace --all-features
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Use the underlying command (or `rtk bypass <command>` / deprecated `rtk proxy <command>`, which runs **without filtering** and only tracks usage) when RTK has no compatible proxy or when exact, unfiltered output is required. On failure, inspect RTK's retained full output before rerunning the command.

```bash
rtk gain
rtk gain --history
```
