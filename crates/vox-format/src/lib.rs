//! vox-format: how a Vox world is stored and sent (see `docs/design/world-format.md`).
//!
//! - [`region`]: a region (16 × 16 chunk columns, 256 m across at half-metre blocks) as one
//!   compressed blob: its edited chunks in vox-core's run-length encoding, compressed together,
//!   with a hash that is its address.
//! - [`store`]: where blobs live (a folder now; object storage behind the same trait), and
//!   snapshots: manifests of region hashes, kept for a week, restorable.
//!
//! Phase 1 holds blocks only; entities, structures and levels as files come next.

pub mod region;
pub mod store;

pub use region::{Region, RegionKey, region_of};
pub use store::{BlobStore, DirStore, Hash, Manifest, Snapshots};

/// A blob or manifest that can't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

pub(crate) fn err<T>(s: impl Into<String>) -> Result<T, Error> {
    Err(Error(s.into()))
}
