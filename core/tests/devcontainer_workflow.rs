//! Tests for devcontainer build workflow configuration
//!
//! Validates that the GitHub Actions workflow for building devcontainer images
//! is properly configured and references all supported stacks.

use regex::Regex;
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

    // Use regex to find the matrix stack definition more robustly
    // Matches patterns like "stack: [rust, rails, nodejs, generic]" with flexible whitespace
    let matrix_regex =
        Regex::new(r"stack:\s*\[([^\]]+)\]").expect("valid regex for matrix definition");

    let matrix_match = matrix_regex
        .captures(&content)
        .expect("workflow should have a stack matrix definition");

    let matrix_content = matrix_match
        .get(1)
        .expect("matrix should have content")
        .as_str();

    // Verify all required stacks are in the matrix
    for stack in ["rust", "rails", "nodejs", "generic"] {
        assert!(
            matrix_content.contains(stack),
            "workflow matrix should include '{stack}', found: {matrix_content}"
        );
    }

    // Verify GHCR registry is used
    assert!(
        content.contains("ghcr.io"),
        "workflow should push to ghcr.io registry"
    );

    // Verify images are tagged with the expected pattern (uses IMAGE_BASE env var)
    assert!(
        content.contains("IMAGE_BASE }}-${{ matrix.stack }}"),
        "workflow should tag stack images with IMAGE_BASE-matrix.stack pattern"
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
