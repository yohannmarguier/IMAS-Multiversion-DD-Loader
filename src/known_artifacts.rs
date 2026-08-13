//! The one embedded conversion-map artifact this project loads directly
//! (ADR 0004): equilibrium, DD 3.39.0 ⇄ 4.1.1. A future conversion-map
//! generator (CONTEXT.md's "conversion-map generator") replaces this
//! hardcoded lookup with something that covers every IDS and version pair;
//! until then, an `(IDS name, stored DD version, HLI DD version)` triple this
//! lookup does not recognise simply has no artifact available, and the shim
//! treats it exactly like an unknown context: passthrough, no registration,
//! no loss (ADR 0011 decision 1 — silence is earned by mechanism coverage,
//! not by which DD-version pair is involved).

use crate::conversion_map::Direction;
use crate::dd_version::DdVersion;

const EQUILIBRIUM_ARTIFACT: &str = include_str!("../docs/3.39.0--4.1.1.xml");

/// The artifact serving one `(IDS, stored, HLI)` triple, plus the direction
/// that resolves a path expressed in the HLI's own spelling to the stored
/// spelling — the down-conversion direction every ADR 0002 seam needs.
pub(crate) struct ArtifactMatch {
    pub(crate) xml: &'static str,
    pub(crate) direction_to_stored: Direction,
}

/// Looks up the artifact for `ids` connecting `stored` and `hli`, in either
/// order. Returns `None` for any IDS or version pair this project has not
/// hand-authored an artifact for.
pub(crate) fn lookup(ids: &str, stored: &DdVersion, hli: &DdVersion) -> Option<ArtifactMatch> {
    if ids != "equilibrium" {
        return None;
    }
    let v3_39_0: DdVersion = "3.39.0".parse().expect("known release");
    let v4_1_1: DdVersion = "4.1.1".parse().expect("known release");

    if stored == &v3_39_0 && hli == &v4_1_1 {
        // Artifact sides: left = 3.39.0 (stored), right = 4.1.1 (HLI). The
        // HLI's own (right) spelling resolves to the stored (left) spelling
        // in reverse.
        Some(ArtifactMatch {
            xml: EQUILIBRIUM_ARTIFACT,
            direction_to_stored: Direction::Reverse,
        })
    } else if stored == &v4_1_1 && hli == &v3_39_0 {
        // Artifact sides: left = 3.39.0 (HLI), right = 4.1.1 (stored). The
        // HLI's own (left) spelling resolves to the stored (right) spelling
        // forward.
        Some(ArtifactMatch {
            xml: EQUILIBRIUM_ARTIFACT,
            direction_to_stored: Direction::Forward,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(input: &str) -> DdVersion {
        input.parse().expect("test DD version must be valid")
    }

    #[test]
    fn matches_the_equilibrium_pair_in_either_direction() {
        let v3 = version("3.39.0");
        let v4 = version("4.1.1");
        assert!(lookup("equilibrium", &v3, &v4).is_some());
        assert!(lookup("equilibrium", &v4, &v3).is_some());
    }

    #[test]
    fn no_artifact_for_an_unknown_ids() {
        let v3 = version("3.39.0");
        let v4 = version("4.1.1");
        assert!(lookup("core_profiles", &v3, &v4).is_none());
    }

    #[test]
    fn no_artifact_for_an_unrelated_version_pair() {
        let v3 = version("3.39.0");
        let other = version("3.40.0");
        assert!(lookup("equilibrium", &v3, &other).is_none());
    }

    #[test]
    fn a_matching_pair_is_not_looked_up_by_callers_but_would_report_none_direction_needed() {
        // Sanity: identical versions on both sides never occur in practice
        // (the registry never calls `lookup` for a matching pair), but the
        // function itself has no special-case for it — it simply finds no
        // (stored, hli) branch and returns `None`, which is the safe default.
        let v4 = version("4.1.1");
        assert!(lookup("equilibrium", &v4, &v4).is_none());
    }
}
