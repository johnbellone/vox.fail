//! How much a device draws and keeps: one profile each for the desktop app, a browser on a
//! computer, and a phone or tablet (docs/design/chunk-streaming.md, *Budgets*).
//!
//! The desktop picks its own; in the browser the page says which (`Start::detail`), since only
//! the page can tell a phone from a laptop. Systems read the [`Detail`] resource, or the
//! platform's default when an app doesn't insert one (the avatar app).

use bevy::prelude::*;

/// Which kind of device this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// the desktop app: a real GPU, threads, gigabytes to spare
    Desktop,
    /// a browser on a computer: one thread, a wasm heap of a few hundred MB to be safe
    Browser,
    /// a browser on a phone or tablet: little memory, a small screen, a battery
    Mobile,
}

impl Profile {
    /// A profile by name (`desktop`, `browser`, `mobile`), as the page sends it.
    pub fn parse(name: &str) -> Option<Profile> {
        match name.trim().to_ascii_lowercase().as_str() {
            "desktop" => Some(Profile::Desktop),
            "browser" => Some(Profile::Browser),
            "mobile" => Some(Profile::Mobile),
            _ => None,
        }
    }

    /// What this build runs as when nobody says: the desktop app natively, a computer's
    /// browser in wasm.
    pub fn native() -> Profile {
        if cfg!(target_arch = "wasm32") { Profile::Browser } else { Profile::Desktop }
    }

    pub fn detail(self) -> Detail {
        match self {
            Profile::Desktop => Detail { profile: self, view_m: 128.0, lod_rings: 3, echo_m: 64.0, mesh_budget_ms: 8 },
            Profile::Browser => Detail { profile: self, view_m: 64.0, lod_rings: 3, echo_m: 48.0, mesh_budget_ms: 5 },
            Profile::Mobile => Detail { profile: self, view_m: 32.0, lod_rings: 2, echo_m: 24.0, mesh_budget_ms: 3 },
        }
    }
}

/// The numbers a profile gives. Distances are metres, so they hold at any block size.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Detail {
    pub profile: Profile,
    /// full detail out to this far: what streaming will ask for (chunk-streaming.md §3, §4)
    pub view_m: f32,
    /// coarser rings past it, each twice as far and twice as coarse (§5)
    pub lod_rings: u8,
    /// a portal's echo holds the destination this far round its arrival point
    pub echo_m: f32,
    /// meshing may take this long each frame (at least one chunk always goes)
    pub mesh_budget_ms: u32,
}

impl Default for Detail {
    fn default() -> Self {
        Profile::native().detail()
    }
}

impl Detail {
    /// The resource if the app has one, else the platform's default.
    pub fn of(d: Option<&Detail>) -> Detail {
        d.copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_by_name_and_smaller_on_smaller_devices() {
        assert_eq!(Profile::parse(" Mobile "), Some(Profile::Mobile));
        assert_eq!(Profile::parse("browser"), Some(Profile::Browser));
        assert_eq!(Profile::parse("toaster"), None);
        let [d, b, m] = [Profile::Desktop, Profile::Browser, Profile::Mobile].map(Profile::detail);
        for (big, small) in [(d, b), (b, m)] {
            assert!(big.view_m >= small.view_m && big.echo_m >= small.echo_m && big.mesh_budget_ms >= small.mesh_budget_ms);
            assert!(big.lod_rings >= small.lod_rings);
        }
        assert_eq!(Detail::default().profile, Profile::native());
    }
}
