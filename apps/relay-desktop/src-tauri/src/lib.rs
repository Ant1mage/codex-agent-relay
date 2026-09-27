//! Relay's Tauri desktop shell.
//!
//! One menu bar item plus the control panel it opens, with no business logic: the
//! shell supervises relayd, projects the daemon's snapshot into a native menu,
//! opens the inspector and the panel in a browser/webview, and drives updates and
//! the clipboard. Everything it shows or changes goes through relayd over HTTP.
//!
//! The modules are deliberately free of AppKit and of the Tauri event loop where
//! they can be — the menu model, the panel URL builder and the daemon's identity
//! rule are unit tested without a GUI at all.
//!
//! The port follows `apps/menu-bar`: `menu-model.ts` → [`menu_model`],
//! `daemon.ts` → [`daemon`], `panel.ts`/`panel-target.ts` → [`panel`],
//! `updater.ts` → [`updater`], `main.ts` → [`shell`] and [`tray`].

pub mod api;
pub mod daemon;
pub mod i18n;
pub mod menu_model;
pub mod panel;
pub mod shell;
pub mod tray;
pub mod updater;

pub use shell::run;
