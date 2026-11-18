//! Stable diff API coverage for conversion impacts.

use std::path::Path;

#[test]
fn compare_with_impacts_matches_compare_on_the_versions_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/versions/wit");
    let old = root.join("v0.1");
    let new = root.join("v0.2");

    let report = witforge::diff::compare(&old, &new).unwrap();
    let analysis = witforge::diff::compare_with_impacts(&old, &new).unwrap();

    assert_eq!(analysis.report, report);
    assert!(analysis.conversion_impacts.iter().any(|impact| {
        impact.type_path == "types/user"
            && impact.missing_member == "old-field"
            && impact.missing_path == "types/user.old-field"
            && impact.from_version == analysis.report.new.version
            && impact.to_version == analysis.report.old.version
    }));
}
