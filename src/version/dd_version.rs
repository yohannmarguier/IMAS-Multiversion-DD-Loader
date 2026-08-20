//! DD-version value type (ADR 0009).
//!
//! Accepts exactly the released (`MAJOR.MINOR.PATCH`) and development
//! (`MAJOR.MINOR.PATCH-N-gHASH`) forms the ADR fixes, against the known
//! 35-version DD chain (`3.22.0` … `4.1.1`). Comparison follows the same
//! ADR: released versions order as numeric triples, a development version
//! sorts after its base release without ever being snapped to it, and two
//! development versions compare equal only on complete string identity —
//! every other development-version pair is incomparable.
//!
//! Not yet wired into any ABI seam — that lands with the discovery and
//! setter work in later tickets under #43.
#![allow(dead_code)]

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

// The known DD version chain, oldest to newest. Sourced from the DD's own
// release tags (imas-dd MCP server / IMAS-Data-Dictionary repo tags).
const KNOWN_RELEASES: &[(u32, u32, u32)] = &[
    (3, 22, 0),
    (3, 23, 0),
    (3, 23, 1),
    (3, 23, 2),
    (3, 23, 3),
    (3, 24, 0),
    (3, 25, 0),
    (3, 26, 0),
    (3, 27, 0),
    (3, 28, 0),
    (3, 28, 1),
    (3, 29, 0),
    (3, 30, 0),
    (3, 31, 0),
    (3, 32, 0),
    (3, 32, 1),
    (3, 33, 0),
    (3, 34, 0),
    (3, 35, 0),
    (3, 36, 0),
    (3, 37, 0),
    (3, 37, 1),
    (3, 37, 2),
    (3, 38, 0),
    (3, 38, 1),
    (3, 39, 0),
    (3, 40, 0),
    (3, 40, 1),
    (3, 41, 0),
    (3, 42, 0),
    (3, 42, 1),
    (3, 42, 2),
    (4, 0, 0),
    (4, 1, 0),
    (4, 1, 1),
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum DdVersion {
    Released {
        major: u32,
        minor: u32,
        patch: u32,
    },
    /// `raw` is the complete, exact input string: two development versions
    /// are equal only when it matches verbatim (ADR 0009).
    Development {
        base: (u32, u32, u32),
        raw: String,
    },
}

impl DdVersion {
    fn sort_key(&self) -> (u32, u32, u32, u8) {
        match self {
            DdVersion::Released {
                major,
                minor,
                patch,
            } => (*major, *minor, *patch, 0),
            DdVersion::Development { base, .. } => (base.0, base.1, base.2, 1),
        }
    }
}

impl fmt::Display for DdVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DdVersion::Released {
                major,
                minor,
                patch,
            } => write!(f, "{major}.{minor}.{patch}"),
            DdVersion::Development { raw, .. } => write!(f, "{raw}"),
        }
    }
}

/// Released versions and a development version's base compare as numeric
/// triples. Two development versions compare equal only when their raw
/// strings match verbatim; any other development-version pair is
/// unmappable (`None`), never guessed from a Git hash or commit distance.
impl PartialOrd for DdVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if let (DdVersion::Development { raw: a, .. }, DdVersion::Development { raw: b, .. }) =
            (self, other)
        {
            return (a == b).then_some(Ordering::Equal);
        }
        Some(self.sort_key().cmp(&other.sort_key()))
    }
}

impl FromStr for DdVersion {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.chars().any(char::is_whitespace) {
            return Err(format!("DD version '{input}' must not contain whitespace"));
        }
        match input.split_once('-') {
            None => parse_released(input),
            Some((base_str, rest)) => parse_development(input, base_str, rest),
        }
    }
}

fn parse_released(input: &str) -> Result<DdVersion, String> {
    let (major, minor, patch) = parse_triple(input)?;
    if !KNOWN_RELEASES.contains(&(major, minor, patch)) {
        return Err(format!("'{input}' is not a known DD release"));
    }
    Ok(DdVersion::Released {
        major,
        minor,
        patch,
    })
}

fn parse_development(raw: &str, base_str: &str, rest: &str) -> Result<DdVersion, String> {
    let base = parse_triple(base_str)?;
    if !KNOWN_RELEASES.contains(&base) {
        return Err(format!("'{raw}' has an unknown base release '{base_str}'"));
    }

    let (distance_str, hash_part) = rest
        .split_once('-')
        .ok_or_else(|| format!("'{raw}' is missing the '-N-gHASH' development suffix"))?;

    if !is_canonical_decimal(distance_str) {
        return Err(format!(
            "'{raw}' has a non-canonical development commit distance"
        ));
    }
    if distance_str == "0" {
        return Err(format!(
            "'{raw}' has a zero commit distance, which is not a development build"
        ));
    }

    let hash = hash_part
        .strip_prefix('g')
        .ok_or_else(|| format!("'{raw}' is missing the 'g' hash prefix"))?;
    if !(7..=64).contains(&hash.len()) {
        return Err(format!(
            "'{raw}' hash must be 7 to 64 characters, got {}",
            hash.len()
        ));
    }
    if !hash
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(format!("'{raw}' hash must be lowercase hexadecimal"));
    }

    Ok(DdVersion::Development {
        base,
        raw: raw.to_string(),
    })
}

fn parse_triple(input: &str) -> Result<(u32, u32, u32), String> {
    let mut parts = input.split('.');
    let major = parts
        .next()
        .ok_or_else(|| format!("'{input}' is not MAJOR.MINOR.PATCH"))?;
    let minor = parts
        .next()
        .ok_or_else(|| format!("'{input}' is not MAJOR.MINOR.PATCH"))?;
    let patch = parts
        .next()
        .ok_or_else(|| format!("'{input}' is not MAJOR.MINOR.PATCH"))?;
    if parts.next().is_some() {
        return Err(format!("'{input}' has extra '.'-separated components"));
    }
    Ok((
        parse_component(major, input)?,
        parse_component(minor, input)?,
        parse_component(patch, input)?,
    ))
}

fn parse_component(component: &str, whole: &str) -> Result<u32, String> {
    if !is_canonical_decimal(component) {
        return Err(format!(
            "'{whole}' has a non-canonical version component '{component}'"
        ));
    }
    component
        .parse()
        .map_err(|_| format!("'{whole}' has an out-of-range version component '{component}'"))
}

/// A non-empty run of ASCII digits with no leading zero (unless it is
/// exactly `"0"`) — the spelling every real DD tag and Git commit distance
/// uses. Rejects `""`, `"04"`, `"-1"`, and anything non-numeric.
fn is_canonical_decimal(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_release_is_accepted() {
        assert!("4.1.1".parse::<DdVersion>().is_ok());
    }

    #[test]
    fn a_valid_development_build_is_accepted() {
        assert!("4.1.1-47-g8eaa5f1".parse::<DdVersion>().is_ok());
    }

    #[test]
    fn an_unknown_release_is_rejected() {
        assert!("4.1.2".parse::<DdVersion>().is_err());
    }

    #[test]
    fn a_malformed_development_build_is_rejected() {
        assert!("4.1.1-0-g8eaa5f1".parse::<DdVersion>().is_err());
    }

    #[test]
    fn every_version_in_the_known_35_version_chain_is_accepted() {
        // The 35-version DD chain (3.22.0 … 4.1.1), typed out independently
        // of `KNOWN_RELEASES` so a transcription error in that table shows
        // up here rather than being validated against itself.
        const CHAIN: &[&str] = &[
            "3.22.0", "3.23.0", "3.23.1", "3.23.2", "3.23.3", "3.24.0", "3.25.0", "3.26.0",
            "3.27.0", "3.28.0", "3.28.1", "3.29.0", "3.30.0", "3.31.0", "3.32.0", "3.32.1",
            "3.33.0", "3.34.0", "3.35.0", "3.36.0", "3.37.0", "3.37.1", "3.37.2", "3.38.0",
            "3.38.1", "3.39.0", "3.40.0", "3.40.1", "3.41.0", "3.42.0", "3.42.1", "3.42.2",
            "4.0.0", "4.1.0", "4.1.1",
        ];
        assert_eq!(CHAIN.len(), 35);
        for &text in CHAIN {
            assert!(
                text.parse::<DdVersion>().is_ok(),
                "expected '{text}' to be accepted as a known release"
            );
        }
    }

    #[test]
    fn an_unknown_development_base_release_is_rejected() {
        assert!("4.1.2-47-g8eaa5f1".parse::<DdVersion>().is_err());
    }

    #[test]
    fn a_short_hash_is_rejected() {
        assert!("4.1.1-47-g8eaa5f".parse::<DdVersion>().is_err()); // 6 hex chars
    }

    #[test]
    fn a_seven_character_hash_is_accepted() {
        assert!("4.1.1-47-g8eaa5f1".parse::<DdVersion>().is_ok()); // 7 hex chars
    }

    #[test]
    fn a_sixty_four_character_hash_is_accepted() {
        let hash = "a".repeat(64);
        assert!(format!("4.1.1-47-g{hash}").parse::<DdVersion>().is_ok());
    }

    #[test]
    fn a_sixty_five_character_hash_is_rejected() {
        let hash = "a".repeat(65);
        assert!(format!("4.1.1-47-g{hash}").parse::<DdVersion>().is_err());
    }

    #[test]
    fn an_uppercase_hash_is_rejected() {
        assert!("4.1.1-47-g8EAA5F1".parse::<DdVersion>().is_err());
    }

    #[test]
    fn whitespace_anywhere_is_rejected() {
        assert!(" 4.1.1".parse::<DdVersion>().is_err());
        assert!("4.1.1 ".parse::<DdVersion>().is_err());
        assert!("4.1.1-47-g8eaa5f1 ".parse::<DdVersion>().is_err());
    }

    #[test]
    fn a_v_prefix_is_rejected() {
        assert!("v4.1.1".parse::<DdVersion>().is_err());
    }

    #[test]
    fn a_dirty_suffix_is_rejected() {
        assert!("4.1.1-47-g8eaa5f1-dirty".parse::<DdVersion>().is_err());
    }

    #[test]
    fn a_pep_440_form_is_rejected() {
        assert!("4.1.1.dev47".parse::<DdVersion>().is_err());
    }

    #[test]
    fn a_bare_git_hash_is_rejected() {
        assert!("8eaa5f1".parse::<DdVersion>().is_err());
    }

    #[test]
    fn negative_one_is_rejected() {
        assert!("-1".parse::<DdVersion>().is_err());
    }

    #[test]
    fn arbitrary_text_is_rejected() {
        assert!("not-a-version".parse::<DdVersion>().is_err());
    }

    #[test]
    fn a_leading_zero_version_component_is_rejected() {
        assert!("04.1.1".parse::<DdVersion>().is_err());
        assert!("4.01.1".parse::<DdVersion>().is_err());
        assert!("4.1.01".parse::<DdVersion>().is_err());
    }

    #[test]
    fn a_leading_zero_commit_distance_is_rejected() {
        assert!("4.1.1-007-g8eaa5f1".parse::<DdVersion>().is_err());
    }

    #[test]
    fn a_development_build_with_a_distance_larger_than_u64_is_accepted() {
        assert!(
            "4.1.1-18446744073709551616-g8eaa5f1"
                .parse::<DdVersion>()
                .is_ok()
        );
    }

    #[test]
    fn a_zero_version_component_is_accepted() {
        assert!("4.0.0".parse::<DdVersion>().is_ok());
    }

    #[test]
    fn released_versions_compare_as_numeric_triples() {
        let earlier: DdVersion = "3.22.0".parse().unwrap();
        let later: DdVersion = "4.1.1".parse().unwrap();
        assert!(earlier < later);
        assert!(later > earlier);

        let same_major_minor_earlier_patch: DdVersion = "3.23.0".parse().unwrap();
        let same_major_minor_later_patch: DdVersion = "3.23.3".parse().unwrap();
        assert!(same_major_minor_earlier_patch < same_major_minor_later_patch);
    }

    #[test]
    fn a_development_version_sorts_after_its_base_release_but_is_never_equal_to_it() {
        let base: DdVersion = "4.1.1".parse().unwrap();
        let dev: DdVersion = "4.1.1-47-g8eaa5f1".parse().unwrap();
        assert!(dev > base);
        assert_ne!(dev, base);
    }

    #[test]
    fn identical_development_strings_are_equal() {
        let a: DdVersion = "4.1.1-47-g8eaa5f1".parse().unwrap();
        let b: DdVersion = "4.1.1-47-g8eaa5f1".parse().unwrap();
        assert_eq!(a, b);
        assert_eq!(a.partial_cmp(&b), Some(Ordering::Equal));
    }

    #[test]
    fn distinct_development_versions_are_unmappable_even_with_the_same_base() {
        let a: DdVersion = "4.1.1-47-g8eaa5f1".parse().unwrap();
        let b: DdVersion = "4.1.1-48-g1234567".parse().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.partial_cmp(&b), None);
    }

    #[test]
    fn distinct_development_versions_on_different_bases_are_also_unmappable() {
        let a: DdVersion = "4.1.1-47-g8eaa5f1".parse().unwrap();
        let b: DdVersion = "4.0.0-3-g1234567".parse().unwrap();
        assert_eq!(a.partial_cmp(&b), None);
    }
}
