//! Runtime resolution of the bundled SpektraFilm asset directories.
//!
//! PhotoCraft deploys the assets as `<exe dir>/spektra/` (repo: `assets/spektra/`):
//! `oracle/` holds the oracle static tables (cmfs/illuminants/spectra LUT/matrices),
//! `profiles/` the 28 upstream film/paper profiles. The environment variable
//! `PHOTOCRAFT_SPEKTTRA_ASSETS` (or `PHOTOCRAFT_SPEKTRA_ASSETS`) overrides the root
//! that contains both subdirectories.

use std::path::PathBuf;

/// The asset root: env override, else `<exe dir>/spektra`, else `./spektra`.
pub fn assets_root() -> PathBuf {
    for var in ["PHOTOCRAFT_SPEKTTRA_ASSETS", "PHOTOCRAFT_SPEKTRA_ASSETS"] {
        if let Ok(dir) = std::env::var(var) {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("spektra");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("spektra")
}

/// Directory holding the oracle static tables.
pub fn oracle_dir() -> PathBuf {
    assets_root().join("oracle")
}

/// Directory holding the film/paper profile JSONs.
///
/// The deployment layout keeps them under `oracle/profiles/` (as in the Android APK assets);
/// a root-level `profiles/` is also accepted for layouts that split the two.
pub fn profiles_dir() -> PathBuf {
    let root = assets_root();
    let direct = root.join("profiles");
    if direct.is_dir() {
        direct
    } else {
        root.join("oracle").join("profiles")
    }
}
