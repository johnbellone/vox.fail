//! vox-format: how a Vox world is stored and sent (see `docs/design/world-format.md`).
//!
//! - [`zone`]: a zone (16 × 16 chunk columns, 128 m across at quarter-metre blocks) as one
//!   compressed blob: its edited chunks in vox-core's run-length encoding, and its entities
//!   (portals, spawns, objectives, settings) as postcard records, each section compressed
//!   separately, with a hash that is its address.
//! - [`region`]: a region (32 × 32 zones, 4,096 m across) as a small blob of its
//!   zones' hashes, itself addressed by hash.
//! - [`store`]: where blobs live (a folder, or an Oxen repository with a commit per snapshot,
//!   behind the same trait; object storage next), and snapshots: manifests of region hashes,
//!   kept for a week, restorable.
//! - [`oxen`]: the Oxen store, with history older than the week squashed away.
//! - [`prop`]: a prop (its `vox-art` definition and voxels) as a `VOXP` blob named by its
//!   version, and the catalogue of accepted props.
//!
//! Zones carry their entities (format 2; format 1 blobs still read). Structures and levels as
//! files come next.

#[cfg(not(target_arch = "wasm32"))]
pub mod oxen;
pub mod prop;
pub mod region;
pub mod store;
pub mod zone;

pub use region::{Region, RegionKey, region_of, region_of_zone};
pub use zone::{Zone, ZoneKey, zone_of};
#[cfg(not(target_arch = "wasm32"))]
pub use oxen::OxenStore;
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
