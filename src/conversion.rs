pub mod conversion_map;
#[cfg(feature = "xml-fixture-source")]
pub(crate) mod known_artifacts;
pub(crate) mod path_conversion;
pub(crate) mod read_outcome;
#[cfg(not(feature = "xml-fixture-source"))]
pub(crate) mod runtime_map;
pub(crate) mod seam_policy;
