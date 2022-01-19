#![doc(html_logo_url = "https://raw.githubusercontent.com/prokopyl/clack/main/logo.svg")]

//! A small crate containing various CLAP utilities and definitions that are common to both
//! plugins and hosts.
//!
//! All modules of this crate are re-exported in the `clack-host` and `clack-plugin` crates. Most users
//! should not have to use `clack-common` directly.

pub mod entry;
pub mod events;
pub mod extensions;
pub mod id;
pub mod ports;
pub mod process;
pub mod stream;
