//! Ordinary-run browser admission. Runtime scopes are never control-plane data.
pub(crate) mod config;
mod files;
mod records;
mod service;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod transport;

pub(crate) use records::*;
pub(crate) use service::{
    BrowserDispatchStamp, BrowserService, PreparedBrowserSelection, UnavailableBrowserPort,
};

mod profiles;
pub(crate) use profiles::{
    BrowserProfileEnvelope, BrowserProfileMutation, BrowserProfileView, BrowserProfilesEnvelope,
    CreateBrowserProfile, StartBrowserLogin,
};
