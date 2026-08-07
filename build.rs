use std::fs;

fn main() {
    const VERSION_FILE: &str = "IMAS_CORE_VERSION";

    println!("cargo::rerun-if-changed={VERSION_FILE}");
    let version = fs::read_to_string(VERSION_FILE)
        .expect("IMAS_CORE_VERSION must contain the supported IMAS-Core release");
    let version = version.trim();
    assert!(
        is_release_version(version),
        "IMAS_CORE_VERSION must have numeric major.minor.patch components"
    );
    println!("cargo::rustc-env=IMAS_CORE_VERSION={version}");
}

fn is_release_version(version: &str) -> bool {
    let mut components = version.split('.');
    let valid = (&mut components).take(3).all(|component| {
        !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
    });
    valid && components.next().is_none() && version.matches('.').count() == 2
}
