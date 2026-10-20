//! Self-update: find a newer release on GitHub, prove it is ours, and put it in place.
//!
//! An update is installed only after its archive matches the published SHA-256 *and*
//! its detached minisign signature verifies against [`RELEASE_PUBLIC_KEY`], the key
//! compiled into this binary. Until a real key is committed, [`check`] reports
//! [`UpdateError::NotConfigured`] and callers show "Updates are not configured for
//! this build".

pub mod handoff;
mod install;
mod release;
mod verify;

#[cfg(test)]
mod handoff_tests;
#[cfg(test)]
mod sign_script_tests;
#[cfg(test)]
mod tests;

use std::{fmt, io};

pub use install::{rollback, swap};
pub use release::{check, current_target, download_binary, http_client, ReleaseInfo, REPO};
// The pieces the supervisor reaches only through the functions above.
#[cfg(test)]
pub use install::swap_at;
#[cfg(test)]
pub use release::{check_at, download, download_verified, select_release, MAX_ARCHIVE_BYTES};
pub use verify::{
    extract_binary, release_key_configured, release_trusted_comment, verify, RELEASE_PUBLIC_KEY,
};

#[derive(Debug)]
pub enum UpdateError {
    /// This build carries no usable release key, so no update can be trusted.
    NotConfigured,
    /// No releases are published for the platform this binary was built for.
    Unsupported,
    /// The release server could not be reached or answered with an error.
    Http(String),
    /// A download was larger than its cap.
    TooLarge {
        limit: u64,
    },
    /// The archive does not match its published SHA-256.
    Checksum,
    /// The minisign signature is missing, malformed or not made by the release key.
    Signature,
    /// A genuine signature, but for another release: its trusted comment is not the one
    /// [`release_trusted_comment`] builds for the selected release (for example an
    /// older signed build re-published under a newer tag).
    ReleaseMismatch {
        expected: String,
        signed: String,
    },
    /// The archive is unreadable, unsafe, or holds no `anima` binary.
    Archive(String),
    Io(io::Error),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured => f.write_str("Updates are not configured for this build"),
            Self::Unsupported => f.write_str("Updates are not published for this platform"),
            Self::Http(message) => write!(f, "Could not reach the update server: {message}"),
            Self::TooLarge { limit } => {
                write!(f, "The update download is larger than {limit} bytes")
            }
            Self::Checksum => f.write_str("The update download does not match its checksum"),
            Self::Signature => f.write_str("The update is not signed by the Anima release key"),
            Self::ReleaseMismatch { expected, signed } => write!(
                f,
                "The update's signature is for a different release \
                 (signed for \"{signed}\", expected \"{expected}\")"
            ),
            Self::Archive(message) => write!(f, "The update archive is not usable: {message}"),
            Self::Io(error) => write!(f, "Could not install the update: {error}"),
        }
    }
}

impl std::error::Error for UpdateError {}

impl UpdateError {
    /// Asking again won't change the answer (until another build is installed).
    pub fn is_permanent(&self) -> bool {
        matches!(self, Self::NotConfigured | Self::Unsupported)
    }
}

impl From<io::Error> for UpdateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<reqwest::Error> for UpdateError {
    fn from(error: reqwest::Error) -> Self {
        Self::Http(error.to_string())
    }
}
