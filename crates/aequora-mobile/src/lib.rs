//! Mobile lifecycle, platform scheduling, and binding contracts.

pub mod runtime;

pub mod bindings;

#[cfg(any(target_os = "android", test, feature = "platform-test-support"))]
pub mod android;

#[cfg(any(target_os = "ios", test, feature = "platform-test-support"))]
pub mod ios;
