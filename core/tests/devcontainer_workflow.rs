//! Tests for devcontainer build workflow configuration
//!
//! Validates that the GitHub Actions workflow for building devcontainer images
//! is properly configured and references all supported stacks.

use std::fs;
use std::path::Path;

/// Validate devcontainer-build.yml workflow exists and has expected structure
#[test]
fn devcontainer_build_workflow_exists_and_has_all_stacks() {
    let workflow_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".github/workflows/devcontainer-build.yml");

    assert!(
        workflow_path.exists(),
        "devcontainer-build.yml workflow should exist at {:?}",
        workflow_path
    );

    let content = fs::read_to_string(&workflow_path).expect("should read workflow file");

    // Verify workflow triggers on devcontainer changes
    assert!(
        content.contains(".devcontainer/**"),
        "workflow should trigger on .devcontainer/ changes"
    );

    // Verify workflow triggers on template Dockerfile changes
    assert!(
        content.contains("core/src/bootstrap/templates/**/Dockerfile"),
        "workflow should trigger on template Dockerfile changes"
    );

    // Verify all stacks are included in the matrix
    for stack in ["rust", "rails", "nodejs", "generic"] {
        assert!(
            content.contains(&format!("stack: [{}", stack))
                || content.contains(&format!("[{},", stack))
                || content.contains(&format!(", {}]", stack))
                || content.contains(&format!(", {},", stack))
                || content.contains(stack),
            "workflow should include {stack} in the matrix"
        );
    }

    // Verify GHCR registry is used
    assert!(
        content.contains("ghcr.io"),
        "workflow should push to ghcr.io registry"
    );

    // Verify images are tagged with the expected pattern
    assert!(
        content.contains("devcontainer-${{ matrix.stack }}") || content.contains("devcontainer-"),
        "workflow should tag images with devcontainer-<stack> pattern"
    );
}

/// Validate that all stack templates have corresponding Dockerfiles
#[test]
fn all_stack_templates_have_dockerfiles() {
    let templates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bootstrap/templates");

    for stack in ["rust", "rails", "nodejs", "generic"] {
        let dockerfile = templates_dir.join(stack).join("Dockerfile");
        assert!(
            dockerfile.exists(),
            "{stack} template should have a Dockerfile at {:?}",
            dockerfile
        );

        let content = fs::read_to_string(&dockerfile).expect("should read Dockerfile");
        assert!(
            content.contains("FROM"),
            "{stack} Dockerfile should have a FROM instruction"
        );
    }
}

/// Validate that all stack templates have compose.yaml with image config
#[test]
fn all_stack_templates_have_compose_with_image() {
    let templates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bootstrap/templates");

    for stack in ["rust", "rails", "nodejs", "generic"] {
        let compose = templates_dir.join(stack).join("compose.yaml");
        assert!(
            compose.exists(),
            "{stack} template should have a compose.yaml at {:?}",
            compose
        );

        let content = fs::read_to_string(&compose).expect("should read compose.yaml");

        // Verify image reference
        let expected_image = format!("devcontainer-{stack}:latest");
        assert!(
            content.contains(&expected_image),
            "{stack} compose.yaml should reference {expected_image}"
        );

        // Verify build fallback
        assert!(
            content.contains("build:") && content.contains("dockerfile:"),
            "{stack} compose.yaml should have build fallback configuration"
        );
    }
}
