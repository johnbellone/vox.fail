//! A prop as one blob (`VOXP`): its definition and its voxels, named by the prop's version, and
//! the catalogue of accepted props that a zone's references are checked against (see
//! `docs/design/props.md`, "Library and shipping").
//!
//! ```text
//! "VOXP"        magic
//! u16           format version: 1
//! u32           definition length, then the definition as JSON (a `vox_art::PropDef`, at most
//!               64 KiB)
//! u32           voxel count (at most 64 × 64 × 64)
//! [u8]          zstd frame, the rest of the blob: for each voxel, in grid order, u16 x, y, z
//!               (little endian) then the palette index (1 to 255; 0 is empty and never stored)
//! ```
//!
//! A blob's name is the prop's version ([`vox_art::PropDef::version`]): BLAKE3 over the
//! canonical definition and its voxels. So the same prop always has the same name, and a
//! changed voxel or a changed field makes a new one. Decoding recomputes that name, so a blob
//! read under the wrong name is refused. Decoding is safe on untrusted input: every length is
//! checked before it is used, decompression stops at a cap, and the definition must pass
//! [`vox_art::PropDef::validate`]. Encoding uses the native zstd library and is not built for
//! wasm; decoding uses the pure Rust decoder that zones use.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use vox_art::{PropDef, VoxelGrid, grid::GridError, prop::Reason};

use crate::{Error, err, store::Hash};

/// The blob's first four bytes.
const MAGIC: &[u8; 4] = b"VOXP";
/// The only format this build writes or reads.
const FORMAT_VERSION: u16 = 1;
/// Largest definition JSON a blob may carry, in bytes.
const MAX_DEF_JSON: usize = 64 << 10;
/// Most voxels a prop may hold: a 64 × 64 × 64 grid, the largest a prop scale allows.
const MAX_VOXELS: u32 = 64 * 64 * 64;
/// Bytes of one voxel record: x, y, z as u16 and the palette index as u8.
const VOXEL_BYTES: usize = 7;
/// Largest catalogue JSON, in bytes.
const MAX_CATALOGUE_BYTES: usize = 1 << 20;
/// Most entries a catalogue may list.
const MAX_CATALOGUE_ENTRIES: usize = 10_000;
// The limits a catalogue entry shares with `vox_art::PropDef::validate` (`crates/vox-art/src/prop.rs`:
// `MAX_ID`, `MAX_NAME`, `MAX_TAGS`, `MAX_TAG`, and the id's characters). A catalogue entry is not
// a PropDef, so the rules are repeated here; keep the two in step.
const MAX_ID: usize = 64;
const MAX_NAME: usize = 64;
const MAX_TAGS: usize = 16;
const MAX_TAG: usize = 32;

/// A prop: its definition and its voxels. Its name is [`PropBlob::hash`].
#[derive(Debug, Clone, PartialEq)]
pub struct PropBlob {
    pub def: PropDef,
    pub grid: VoxelGrid,
}

impl PropBlob {
    /// The prop's name: its version, as a [`Hash`]. Fails if the grid is not the def's size.
    pub fn hash(&self) -> Result<Hash, Error> {
        self.def.version(&self.grid).map(Hash).map_err(reason_error)
    }

    /// The blob and its name. The def must validate, and the grid must be the def's size.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn encode(&self) -> Result<(Vec<u8>, Hash), Error> {
        self.def.validate().map_err(reasons_error)?;
        let hash = self.hash()?;
        let json = serde_json::to_vec(&self.def).map_err(|e| Error(format!("prop definition: {e}")))?;
        if json.len() > MAX_DEF_JSON {
            return err("the prop's definition is too big");
        }
        let voxels: Vec<([u16; 3], u8)> = self.grid.iter().collect();
        if voxels.len() > MAX_VOXELS as usize {
            return err("too many voxels for a prop");
        }
        let mut raw = Vec::with_capacity(voxels.len() * VOXEL_BYTES);
        for ([x, y, z], index) in &voxels {
            raw.extend_from_slice(&x.to_le_bytes());
            raw.extend_from_slice(&y.to_le_bytes());
            raw.extend_from_slice(&z.to_le_bytes());
            raw.push(*index);
        }
        let packed = zstd::bulk::compress(&raw, 9)?;
        let def_len = u32::try_from(json.len()).map_err(|_| Error("the prop's definition is too big".into()))?;
        let count = u32::try_from(voxels.len()).map_err(|_| Error("too many voxels for a prop".into()))?;
        let mut out = Vec::with_capacity(4 + 2 + 4 + json.len() + 4 + packed.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&def_len.to_le_bytes());
        out.extend_from_slice(&json);
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&packed);
        Ok((out, hash))
    }

    /// Read a blob (from storage or from anyone). Checked, never panics; the def is validated.
    pub fn decode(blob: &[u8]) -> Result<PropBlob, Error> {
        let mut pos = 0;
        if take(blob, &mut pos, 4)? != MAGIC {
            return err("not a prop");
        }
        let version = u16::from_le_bytes(take_array(blob, &mut pos)?);
        if version != FORMAT_VERSION {
            return err(format!("prop format {version}, this build reads {FORMAT_VERSION}"));
        }
        let def_len = u32::from_le_bytes(take_array(blob, &mut pos)?) as usize;
        if def_len > MAX_DEF_JSON {
            return err("the prop's definition is too big");
        }
        let json = take(blob, &mut pos, def_len)?;
        let def: PropDef = serde_json::from_slice(json).map_err(|e| Error(format!("prop definition: {e}")))?;
        def.validate().map_err(reasons_error)?;
        let count = u32::from_le_bytes(take_array(blob, &mut pos)?);
        if count > MAX_VOXELS {
            return err("too many voxels for a prop");
        }
        let cap = (count as usize).checked_mul(VOXEL_BYTES).ok_or_else(|| Error("too many voxels for a prop".into()))?;
        let frame = blob.get(pos..).ok_or_else(|| Error("the prop is cut short".into()))?;
        let raw = unzstd_frame(frame, cap)?;
        if raw.len() != cap {
            return err(format!("the voxel data holds {} bytes, not {cap} for {count} voxels", raw.len()));
        }
        let mut grid = VoxelGrid::new(def.scale.scale(), def.size).map_err(grid_error)?;
        // Records must come in strictly increasing (x, y, z) order, the order `encode` writes
        // them in. That one rule also refuses a voxel listed twice.
        let mut previous: Option<[u16; 3]> = None;
        // the length was checked to be exactly count × 7, so there is no remainder to drop
        for &[x0, x1, y0, y1, z0, z1, index] in raw.as_chunks::<VOXEL_BYTES>().0 {
            let pos = [u16::from_le_bytes([x0, x1]), u16::from_le_bytes([y0, y1]), u16::from_le_bytes([z0, z1])];
            if previous.is_some_and(|p| pos <= p) {
                return err(format!("the voxel at {pos:?} is out of order or listed twice"));
            }
            previous = Some(pos);
            if index == 0 {
                return err(format!("the voxel at {pos:?} is empty; empty voxels are not stored"));
            }
            grid.set(pos, index).map_err(grid_error)?;
        }
        Ok(PropBlob { def, grid })
    }

    /// Read a blob and check that it is the prop named `expected`; a blob under another name is
    /// refused.
    pub fn decode_named(bytes: &[u8], expected: &Hash) -> Result<PropBlob, Error> {
        let blob = PropBlob::decode(bytes)?;
        let got = blob.hash()?;
        if got != *expected {
            return err(format!("the blob is prop {}, not {}", got.hex(), expected.hex()));
        }
        Ok(blob)
    }
}

/// The next `n` bytes, advancing the position; refused (not panicked) past the end.
fn take<'a>(b: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], Error> {
    let end = pos.checked_add(n).ok_or_else(|| Error("the prop is cut short".into()))?;
    let s = b.get(*pos..end).ok_or_else(|| Error("the prop is cut short".into()))?;
    *pos = end;
    Ok(s)
}

fn take_array<const N: usize>(b: &[u8], pos: &mut usize) -> Result<[u8; N], Error> {
    take(b, pos, N)?.try_into().map_err(|_| Error("the prop is cut short".into()))
}

fn reason_error(r: Reason) -> Error {
    Error(format!("{}: {}", r.field, r.problem))
}

fn reasons_error(rs: Vec<Reason>) -> Error {
    Error(rs.into_iter().map(|r| format!("{}: {}", r.field, r.problem)).collect::<Vec<_>>().join("; "))
}

fn grid_error(e: GridError) -> Error {
    Error(e.to_string())
}

/// Decompresses the blob's zstd frame, refusing more than `cap` bytes, a cut-short frame, and
/// any bytes after the frame. The streaming decoder reads its source only as far as the frame
/// goes, so whatever is left unread is the trailing data. This is the zone reader's `unzstd`
/// with that last check.
fn unzstd_frame(frame: &[u8], cap: usize) -> Result<Vec<u8>, Error> {
    use std::io::Read;
    let mut dec = ruzstd::decoding::StreamingDecoder::new(frame).map_err(|e| Error(format!("zstd: {e}")))?;
    let mut out = Vec::new();
    let read = (&mut dec).take(cap as u64 + 1).read_to_end(&mut out).map_err(|e| Error(format!("zstd: {e}")))?;
    if read > cap {
        return err("decompresses to too much");
    }
    if !dec.decoder.is_finished() {
        return err("the zstd frame is cut short");
    }
    if !dec.get_ref().is_empty() {
        return err("the prop has bytes after its zstd frame");
    }
    Ok(out)
}

/// The limits on one catalogue entry, the same as `PropDef::validate` gives a def: its id (the
/// map key) and its name, tags and tag count. Text must be 1 to its cap in characters and have
/// no control characters.
fn check_entry(id: &str, entry: &CatalogueEntry) -> Result<(), Error> {
    let id_len = id.chars().count();
    let id_ok = id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    if !(1..=MAX_ID).contains(&id_len) || !id_ok {
        return err(format!("catalogue id {id:?} is not a prop id"));
    }
    if !short_text(&entry.name, MAX_NAME) {
        return err(format!("catalogue prop {id}: name must be 1 to {MAX_NAME} characters, with no control characters"));
    }
    if entry.tags.len() > MAX_TAGS {
        return err(format!("catalogue prop {id}: more than {MAX_TAGS} tags"));
    }
    for tag in &entry.tags {
        if !short_text(tag, MAX_TAG) {
            return err(format!("catalogue prop {id}: tag {tag:?} must be 1 to {MAX_TAG} characters, with no control characters"));
        }
    }
    Ok(())
}

fn short_text(s: &str, max: usize) -> bool {
    (1..=max).contains(&s.chars().count()) && !s.chars().any(char::is_control)
}

/// The accepted props: each def id to the name of its accepted version, with its display name
/// and tags. Serialised as JSON; a zone or a world may name only props listed here.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Catalogue {
    pub props: BTreeMap<String, CatalogueEntry>,
}

/// One accepted prop in a [`Catalogue`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogueEntry {
    /// The prop's name, its version (see [`PropBlob::hash`]).
    #[serde(with = "hash_hex")]
    pub hash: Hash,
    pub name: String,
    pub tags: Vec<String>,
}

impl std::fmt::Debug for CatalogueEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CatalogueEntry")
            .field("hash", &self.hash.hex())
            .field("name", &self.name)
            .field("tags", &self.tags)
            .finish()
    }
}

impl Catalogue {
    /// The catalogue as JSON. Refused past the entry or size caps.
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        if self.props.len() > MAX_CATALOGUE_ENTRIES {
            return err("too many props in the catalogue");
        }
        // Refused here too, so a catalogue this tool writes is one that `decode` reads.
        for (id, entry) in &self.props {
            check_entry(id, entry)?;
        }
        let json = serde_json::to_vec(self).map_err(|e| Error(format!("catalogue: {e}")))?;
        if json.len() > MAX_CATALOGUE_BYTES {
            return err("the catalogue is too big");
        }
        Ok(json)
    }

    /// Read a catalogue (from storage or from anyone). Refused past the size or entry caps.
    pub fn decode(bytes: &[u8]) -> Result<Catalogue, Error> {
        if bytes.len() > MAX_CATALOGUE_BYTES {
            return err("the catalogue is too big");
        }
        let cat: Catalogue = serde_json::from_slice(bytes).map_err(|e| Error(format!("catalogue: {e}")))?;
        if cat.props.len() > MAX_CATALOGUE_ENTRIES {
            return err("too many props in the catalogue");
        }
        // The whole catalogue is refused on one bad entry: this file is written by our own tool.
        for (id, entry) in &cat.props {
            check_entry(id, entry)?;
        }
        Ok(cat)
    }

    /// Whether some accepted prop is this version.
    pub fn accepts(&self, hash: &Hash) -> bool {
        self.props.values().any(|e| e.hash == *hash)
    }
}

/// A hash as 32 hex digits in JSON.
mod hash_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::store::Hash;

    pub fn serialize<S: Serializer>(hash: &Hash, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hash.hex())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Hash, D::Error> {
        let s = String::deserialize(d)?;
        Hash::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_art::Scale;

    const BOX: &str = r#"{
        "id": "crate.box",
        "name": "Crate",
        "scale": "prop",
        "size": [4, 4, 4],
        "pivot": [1, 0, 1],
        "place": { "on": ["floor"], "footprint": [[0, 0]], "height": 2 },
        "collide": "footprint",
        "palette_version": 1
    }"#;

    fn def_from(json: &str) -> PropDef {
        serde_json::from_str(json).unwrap()
    }

    fn sample() -> PropBlob {
        let def = def_from(BOX);
        let mut grid = VoxelGrid::new(Scale::Prop, def.size).unwrap();
        grid.set([1, 0, 1], 1).unwrap();
        grid.set([2, 2, 2], 3).unwrap();
        PropBlob { def, grid }
    }

    fn rec(x: u16, y: u16, z: u16, index: u8) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend_from_slice(&x.to_le_bytes());
        r.extend_from_slice(&y.to_le_bytes());
        r.extend_from_slice(&z.to_le_bytes());
        r.push(index);
        r
    }

    /// A blob built by hand around an already compressed voxel frame, with any def JSON and any
    /// voxel count.
    fn craft_frame(json: &[u8], count: u32, frame: &[u8]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(json);
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(frame);
        out
    }

    /// A blob built by hand: any def JSON, any voxel count, and any raw voxel records.
    fn craft(json: &[u8], count: u32, raw: &[u8]) -> Vec<u8> {
        craft_frame(json, count, &zstd::bulk::compress(raw, 9).unwrap())
    }

    #[test]
    fn a_prop_round_trips() {
        let p = sample();
        let (blob, hash) = p.encode().unwrap();
        let back = PropBlob::decode(&blob).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.hash().unwrap().hex(), hash.hex(), "the name survives the trip");
        assert_eq!(PropBlob::decode_named(&blob, &hash).unwrap(), p);
    }

    #[test]
    fn a_changed_voxel_changes_the_name() {
        let a = sample();
        let mut b = sample();
        b.grid.set([2, 2, 2], 4).unwrap();
        assert_ne!(a.hash().unwrap().hex(), b.hash().unwrap().hex());
    }

    #[test]
    fn decode_named_refuses_a_wrong_name() {
        let (blob, hash) = sample().encode().unwrap();
        let wrong = Hash([0xab; 16]);
        assert!(wrong != hash);
        assert!(PropBlob::decode_named(&blob, &wrong).is_err());
    }

    #[test]
    fn a_hand_built_blob_is_read_when_it_is_valid() {
        let raw = [rec(1, 0, 1, 1), rec(2, 2, 2, 3)].concat();
        let blob = craft(BOX.as_bytes(), 2, &raw);
        let p = PropBlob::decode(&blob).unwrap();
        assert_eq!(p.grid.get([2, 2, 2]), Some(3));
        assert_eq!(p.grid.len(), 2);
    }

    #[test]
    fn crafted_blobs_are_refused_not_crashed_on() {
        let good = craft(BOX.as_bytes(), 1, &rec(1, 0, 1, 1));
        assert!(PropBlob::decode(&good).is_ok());

        // bad magic
        let mut magic = good.clone();
        magic[0] = b'X';
        assert!(PropBlob::decode(&magic).is_err());

        // a definition length past the end
        let mut past = good.clone();
        past[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(PropBlob::decode(&past).is_err());

        // a definition over the byte cap
        let big = vec![b' '; MAX_DEF_JSON + 1];
        assert!(PropBlob::decode(&craft(&big, 0, &[])).is_err());

        // a voxel count over the cap, with nothing after it
        assert!(PropBlob::decode(&craft(BOX.as_bytes(), MAX_VOXELS + 1, &[])).is_err());

        // a voxel outside the def's size (x = 4 in a size-4 grid)
        assert!(PropBlob::decode(&craft(BOX.as_bytes(), 1, &rec(4, 0, 0, 1))).is_err());

        // an empty voxel (index 0), which is never stored
        assert!(PropBlob::decode(&craft(BOX.as_bytes(), 1, &rec(1, 0, 1, 0))).is_err());

        // the same voxel twice
        let twice = [rec(1, 0, 1, 1), rec(1, 0, 1, 2)].concat();
        assert!(PropBlob::decode(&craft(BOX.as_bytes(), 2, &twice)).is_err());

        // a count that does not match the voxel data
        let two = [rec(1, 0, 1, 1), rec(2, 2, 2, 3)].concat();
        assert!(PropBlob::decode(&craft(BOX.as_bytes(), 1, &two)).is_err());

        // a definition that fails validate (an id with a capital letter)
        assert!(PropBlob::decode(&craft(BOX.replace("crate.box", "Crate Box").as_bytes(), 0, &[])).is_err());

        // a frame that decompresses to more than the count allows
        let bomb = zstd::bulk::compress(&vec![0u8; 1 << 20], 19).unwrap();
        assert!(PropBlob::decode(&craft_frame(BOX.as_bytes(), 1, &bomb)).is_err());

        assert!(PropBlob::decode(b"VOXP").is_err());
        assert!(PropBlob::decode(b"nope").is_err());
    }

    #[test]
    fn every_prefix_of_a_blob_is_refused_or_read_never_a_panic() {
        let (blob, _) = sample().encode().unwrap();
        for n in 0..blob.len() {
            let _ = PropBlob::decode(&blob[..n]);
        }
        assert!(PropBlob::decode(&blob[..blob.len() - 1]).is_err());
    }

    #[test]
    fn catalogue_round_trips_and_accepts_only_listed_names() {
        let (_, hash) = sample().encode().unwrap();
        let mut cat = Catalogue::default();
        cat.props.insert(
            "crate.box".into(),
            CatalogueEntry { hash, name: "Crate".into(), tags: vec!["wood".into(), "small".into()] },
        );
        let json = cat.encode().unwrap();
        let back = Catalogue::decode(&json).unwrap();
        assert_eq!(back.encode().unwrap(), json, "one catalogue, one JSON");
        assert_eq!(back.props["crate.box"].hash.hex(), hash.hex());
        assert!(back.accepts(&hash));
        assert!(!back.accepts(&Hash([0; 16])));
    }

    #[test]
    fn catalogue_refuses_too_many_entries() {
        let (_, hash) = sample().encode().unwrap();
        let mut cat = Catalogue::default();
        for i in 0..=MAX_CATALOGUE_ENTRIES {
            cat.props.insert(format!("p{i}"), CatalogueEntry { hash, name: "x".into(), tags: vec![] });
        }
        assert!(cat.encode().is_err());
        assert!(Catalogue::decode(b"{\"props\":{}}extra").is_err());
    }

    #[test]
    fn voxel_records_out_of_order_are_refused() {
        let backwards = [rec(2, 2, 2, 3), rec(1, 0, 1, 1)].concat();
        assert!(PropBlob::decode(&craft(BOX.as_bytes(), 2, &backwards)).is_err());
        // y before x in the order is also backwards: (0, 3, 0) comes before (1, 0, 0)
        let by_y = [rec(1, 0, 0, 1), rec(0, 3, 0, 1)].concat();
        assert!(PropBlob::decode(&craft(BOX.as_bytes(), 2, &by_y)).is_err());
        // in order, the same two voxels are read
        let sorted = [rec(1, 0, 1, 1), rec(2, 2, 2, 3)].concat();
        assert!(PropBlob::decode(&craft(BOX.as_bytes(), 2, &sorted)).is_ok());
    }

    #[test]
    fn encoded_voxels_are_in_sorted_order_whatever_the_insertion_order() {
        let def = def_from(BOX);
        let mut grid = VoxelGrid::new(Scale::Prop, def.size).unwrap();
        grid.set([2, 2, 2], 3).unwrap();
        grid.set([1, 0, 1], 1).unwrap();
        let (blob, _) = PropBlob { def, grid }.encode().unwrap();
        let back = PropBlob::decode(&blob).unwrap();
        assert_eq!(back.grid.get([1, 0, 1]), Some(1));
        assert_eq!(back.grid.get([2, 2, 2]), Some(3));
    }

    #[test]
    fn bytes_after_the_zstd_frame_are_refused() {
        let raw = rec(1, 0, 1, 1);
        let mut frame = zstd::bulk::compress(&raw, 9).unwrap();
        assert!(PropBlob::decode(&craft_frame(BOX.as_bytes(), 1, &frame)).is_ok());
        frame.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        assert!(PropBlob::decode(&craft_frame(BOX.as_bytes(), 1, &frame)).is_err());
    }

    fn entry(name: &str, tags: Vec<String>) -> CatalogueEntry {
        let (_, hash) = sample().encode().unwrap();
        CatalogueEntry { hash, name: name.into(), tags }
    }

    fn one(id: &str, entry: CatalogueEntry) -> Catalogue {
        let mut cat = Catalogue::default();
        cat.props.insert(id.into(), entry);
        cat
    }

    #[test]
    fn a_catalogue_entry_that_prop_validation_would_refuse_is_refused() {
        // An id with a space and a capital letter.
        let bad_id = one("Crate Box", entry("Crate", vec![]));
        assert!(bad_id.encode().is_err());
        assert!(Catalogue::decode(&serde_json::to_vec(&bad_id).unwrap()).is_err());

        // A name of 65 characters, and an empty name.
        assert!(one("crate.box", entry(&"n".repeat(MAX_NAME + 1), vec![])).encode().is_err());
        assert!(one("crate.box", entry("", vec![])).encode().is_err());

        // A control character in the name.
        assert!(one("crate.box", entry("cr\u{7}ate", vec![])).encode().is_err());

        // Seventeen tags, and a tag of 33 characters.
        let many: Vec<String> = (0..=MAX_TAGS).map(|i| format!("t{i}")).collect();
        assert!(one("crate.box", entry("Crate", many)).encode().is_err());
        assert!(one("crate.box", entry("Crate", vec!["t".repeat(MAX_TAG + 1)])).encode().is_err());

        // Sixteen tags of 32 characters each are at the limit and pass.
        let at_limit: Vec<String> = (0..MAX_TAGS).map(|_| "t".repeat(MAX_TAG)).collect();
        assert!(one("crate.box", entry("Crate", at_limit)).encode().is_ok());
    }

    #[test]
    fn one_bad_entry_refuses_the_whole_catalogue() {
        let (_, hash) = sample().encode().unwrap();
        let mut cat = Catalogue::default();
        cat.props.insert("crate.box".into(), CatalogueEntry { hash, name: "Crate".into(), tags: vec![] });
        cat.props.insert("Bad".into(), CatalogueEntry { hash, name: "Bad".into(), tags: vec![] });
        let json = serde_json::to_vec(&cat).unwrap();
        assert!(Catalogue::decode(&json).is_err());
    }
}
