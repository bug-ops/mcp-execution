//! Acceptance test of the `ProgressiveGenerator` -> `PackageGenerator` rename (#680): the type
//! lives in `package`, is re-exported at the crate root, and generates the same ten-file
//! package as before.

#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod support;

use mcp_execution_codegen::PackageGenerator;
use mcp_execution_codegen::package::PackageGenerator as ModulePathGenerator;
use support::{create_test_server_info, test_config};

#[test]
fn generate_produces_the_full_package_through_both_paths() {
    let server_info = create_test_server_info();

    let code = PackageGenerator::new()
        .expect("create generator")
        .generate(&server_info, &test_config())
        .expect("generate package");
    assert_eq!(code.file_count(), 10);

    let via_module = ModulePathGenerator::new()
        .expect("create generator")
        .generate(&server_info, &test_config())
        .expect("generate package");
    assert_eq!(via_module.file_count(), code.file_count());
}
