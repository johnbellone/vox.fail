//! The project palette: named materials, each a ramp of colours from dark to light.
//!
//! Palette files come from users and tools, so they are capped before they are trusted: at
//! most 64 materials, names of 1 to 32 characters, unique names, 1 to 4 colours per ramp and
//! 255 colours in all. Those limits keep every colour index in one byte, with 0 left for
//! "empty" (see [`crate::grid`]).

use serde::{Deserialize, Serialize};

/// Most materials in a palette.
const MAX_MATERIALS: usize = 64;
/// Longest material name, in characters.
const MAX_NAME_CHARS: usize = 32;
/// Most colours in one material's ramp.
const MAX_RAMP: usize = 4;
/// Most colours in the whole palette: indices 1..=255 fit a `u8`.
const MAX_COLOURS: usize = 255;
/// Largest palette file accepted, in bytes. Well above what the caps allow.
const MAX_JSON_BYTES: usize = 64 * 1024;

/// A palette of named materials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    pub version: u16,
    pub materials: Vec<Material>,
}

/// One named material: a ramp of RGBA colours, dark to light.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    pub colours: Vec<[u8; 4]>,
}

impl Palette {
    /// Reads and checks a palette from JSON.
    pub fn from_json(json: &str) -> Result<Palette, String> {
        if json.len() > MAX_JSON_BYTES {
            return Err(format!("palette is over {MAX_JSON_BYTES} bytes"));
        }
        let palette: Palette =
            serde_json::from_str(json).map_err(|e| format!("palette JSON: {e}"))?;
        palette.validate()?;
        Ok(palette)
    }

    /// The 1-based palette index of tone `tone` (0 = darkest) of material `name`.
    ///
    /// Indices run over every colour in order, material by material, so 0 stays free for
    /// "empty". Returns `None` for an unknown name or a tone the ramp does not have.
    pub fn index_of(&self, name: &str, tone: usize) -> Option<u8> {
        let mut before = 0usize;
        for material in &self.materials {
            if material.name == name {
                if tone >= material.colours.len() {
                    return None;
                }
                return u8::try_from(before.checked_add(tone)?.checked_add(1)?).ok();
            }
            before = before.checked_add(material.colours.len())?;
        }
        None
    }

    /// The RGBA colour of a palette index (1-based, as [`Palette::index_of`] numbers them).
    ///
    /// Returns `None` for 0 ("empty") and for an index past the last colour.
    pub fn colour(&self, index: u8) -> Option<[u8; 4]> {
        let mut next = 1usize;
        for material in &self.materials {
            for colour in &material.colours {
                if next == usize::from(index) {
                    return Some(*colour);
                }
                next += 1;
            }
        }
        None
    }

    fn validate(&self) -> Result<(), String> {
        if self.materials.len() > MAX_MATERIALS {
            return Err(format!("palette has more than {MAX_MATERIALS} materials"));
        }
        let mut names: Vec<&str> = Vec::with_capacity(self.materials.len());
        let mut colours = 0usize;
        for material in &self.materials {
            let chars = material.name.chars().count();
            if chars == 0 || chars > MAX_NAME_CHARS {
                return Err(format!(
                    "material name {:?} must be 1 to {MAX_NAME_CHARS} characters",
                    material.name
                ));
            }
            if names.contains(&material.name.as_str()) {
                return Err(format!("material name {:?} is used twice", material.name));
            }
            names.push(&material.name);
            if material.colours.is_empty() || material.colours.len() > MAX_RAMP {
                return Err(format!(
                    "material {:?} needs 1 to {MAX_RAMP} colours",
                    material.name
                ));
            }
            colours = colours
                .checked_add(material.colours.len())
                .ok_or("palette colour count overflows")?;
        }
        if colours > MAX_COLOURS {
            return Err(format!("palette has more than {MAX_COLOURS} colours"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
        "version": 1,
        "materials": [
            { "name": "stone", "colours": [[60, 60, 64, 255], [120, 120, 128, 255]] },
            { "name": "wood", "colours": [[90, 50, 20, 255], [140, 90, 40, 255], [190, 140, 80, 255]] }
        ]
    }"#;

    #[test]
    fn reads_a_palette_and_indexes_its_colours() {
        let palette = Palette::from_json(JSON).unwrap();
        assert_eq!(palette.version, 1);
        assert_eq!(palette.index_of("stone", 0), Some(1));
        assert_eq!(palette.index_of("stone", 1), Some(2));
        assert_eq!(palette.index_of("wood", 0), Some(3));
        assert_eq!(palette.index_of("wood", 2), Some(5));
        assert_eq!(palette.index_of("wood", 3), None);
        assert_eq!(palette.index_of("glass", 0), None);
    }

    #[test]
    fn rejects_duplicate_names_and_empty_ramps() {
        let dup = JSON.replace("\"wood\"", "\"stone\"");
        assert!(Palette::from_json(&dup).unwrap_err().contains("used twice"));
        let empty = r#"{"version":1,"materials":[{"name":"a","colours":[]}]}"#;
        assert!(Palette::from_json(empty).is_err());
    }

    #[test]
    fn rejects_long_names_and_too_many_colours() {
        let long = format!(
            r#"{{"version":1,"materials":[{{"name":"{}","colours":[[0,0,0,255]]}}]}}"#,
            "x".repeat(MAX_NAME_CHARS + 1)
        );
        assert!(Palette::from_json(&long).is_err());

        let mut materials = Vec::new();
        for i in 0..MAX_MATERIALS + 1 {
            materials.push(format!(r#"{{"name":"m{i}","colours":[[0,0,0,255]]}}"#));
        }
        let many = format!(r#"{{"version":1,"materials":[{}]}}"#, materials.join(","));
        assert!(Palette::from_json(&many).unwrap_err().contains("materials"));
    }

    #[test]
    fn rejects_non_json() {
        assert!(Palette::from_json("not json").is_err());
    }
}
