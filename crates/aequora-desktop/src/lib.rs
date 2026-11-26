//! Desktop lifecycle, agent IPC, and signed update contracts.

pub mod runtime;

pub mod agent;

pub mod ipc;

pub mod update;

#[cfg(any(target_os = "linux", test, feature = "platform-test-support"))]
pub mod linux;

#[cfg(any(target_os = "windows", test, feature = "platform-test-support"))]
pub mod windows;

#[cfg(any(target_os = "macos", test, feature = "platform-test-support"))]
pub mod macos;
