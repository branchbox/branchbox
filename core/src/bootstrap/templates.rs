//! Templates for devcontainer generation
//!
//! Contains template functions for generating devcontainer configuration files.

use super::Stack;
use crate::Result;

/// Generate devcontainer.json template
pub fn devcontainer_json(stack: Stack) -> Result<String> {
    let template = match stack {
        Stack::Rust => include_str!("templates/rust/devcontainer.json"),
        Stack::Rails => include_str!("templates/rails/devcontainer.json"),
        Stack::NodeJs => include_str!("templates/nodejs/devcontainer.json"),
        Stack::Generic => include_str!("templates/generic/devcontainer.json"),
    };

    Ok(template.to_string())
}

/// Generate compose.yaml template
pub fn compose_yaml(stack: Stack) -> Result<String> {
    let template = match stack {
        Stack::Rust => include_str!("templates/rust/compose.yaml"),
        Stack::Rails => include_str!("templates/rails/compose.yaml"),
        Stack::NodeJs => include_str!("templates/nodejs/compose.yaml"),
        Stack::Generic => include_str!("templates/generic/compose.yaml"),
    };

    Ok(template.to_string())
}

/// Generate Dockerfile template
pub fn dockerfile(stack: Stack) -> Result<String> {
    let template = match stack {
        Stack::Rust => include_str!("templates/rust/Dockerfile"),
        Stack::Rails => include_str!("templates/rails/Dockerfile"),
        Stack::NodeJs => include_str!("templates/nodejs/Dockerfile"),
        Stack::Generic => include_str!("templates/generic/Dockerfile"),
    };

    Ok(template.to_string())
}

/// Generate .env.sample template
pub fn env_sample(stack: Stack) -> Result<String> {
    let template = match stack {
        Stack::Rust => include_str!("templates/rust/env.sample"),
        Stack::Rails => include_str!("templates/rails/env.sample"),
        Stack::NodeJs => include_str!("templates/nodejs/env.sample"),
        Stack::Generic => include_str!("templates/generic/env.sample"),
    };

    Ok(template.to_string())
}

/// Generate the placeholder BranchBox env overrides
pub fn branchbox_env() -> Result<String> {
    Ok(include_str!("templates/branchbox.env").to_string())
}

/// Generate the BranchBox quickstart documentation
pub fn branchbox_docs() -> Result<String> {
    Ok(include_str!("templates/BRANCHBOX.md").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_template_includes_workspace_mount_and_shared_configs() {
        let compose = compose_yaml(Stack::Rust).expect("rust compose template");
        assert!(
            compose.contains("../..:/workspaces:cached"),
            "compose template missing workspace bind: {compose}"
        );
        for shared in [
            "${SHARED_CONFIG_DIR:-../..}/.codex:/home/vscode/.codex",
            "${SHARED_CONFIG_DIR:-../..}/.claude:/home/vscode/.claude",
            "${SHARED_CONFIG_DIR:-../..}/.claude.json:/home/vscode/.claude.json",
            "${SHARED_CONFIG_DIR:-../..}/.gh:/home/vscode/.config/gh",
        ] {
            assert!(
                compose.contains(shared),
                "compose template missing shared config volume {shared}: {compose}"
            );
        }
    }

    #[test]
    fn compose_template_includes_prebuilt_image_config() {
        for (stack, image_suffix) in [
            (Stack::Rust, "devcontainer-rust"),
            (Stack::Rails, "devcontainer-rails"),
            (Stack::NodeJs, "devcontainer-nodejs"),
            (Stack::Generic, "devcontainer-generic"),
        ] {
            let compose = compose_yaml(stack).expect("compose template");
            // Verify pre-built image with env var override
            assert!(
                compose.contains("${DEVCONTAINER_IMAGE:-ghcr.io/branchbox/branchbox/"),
                "compose template missing DEVCONTAINER_IMAGE env var: {compose}"
            );
            assert!(
                compose.contains(image_suffix),
                "compose template missing image suffix {image_suffix}: {compose}"
            );
            // Verify pull_policy with env var
            assert!(
                compose.contains("pull_policy: ${DEVCONTAINER_PULL_POLICY:-missing}"),
                "compose template missing pull_policy: {compose}"
            );
            // Verify build fallback is still present
            assert!(
                compose.contains("dockerfile: .devcontainer/Dockerfile"),
                "compose template missing Dockerfile build fallback: {compose}"
            );
        }
    }

    #[test]
    fn branchbox_env_placeholder_has_expected_defaults() {
        let env = branchbox_env().expect("branchbox env template");
        for needle in [
            "WORK_FEATURE=main",
            "BRANCHBOX_MAIN_NAME=main",
            "GIT_BRANCH=main",
        ] {
            assert!(
                env.contains(needle),
                "branchbox.env missing {needle}: {env}"
            );
        }
    }

    #[test]
    fn compose_template_includes_container_behavior_settings() {
        for stack in [Stack::Rust, Stack::Rails, Stack::NodeJs, Stack::Generic] {
            let compose = compose_yaml(stack).expect("compose template");
            // All stacks should have init: true for proper signal handling
            assert!(
                compose.contains("init: true"),
                "{stack:?} compose template missing 'init: true': {compose}"
            );
            // All stacks should have ipc: host for better container performance
            assert!(
                compose.contains("ipc: host"),
                "{stack:?} compose template missing 'ipc: host': {compose}"
            );
        }
    }

    #[test]
    fn env_sample_includes_devcontainer_variables() {
        for stack in [Stack::Rust, Stack::Rails, Stack::NodeJs, Stack::Generic] {
            let env = env_sample(stack).expect("env sample template");
            // All stacks should document DEVCONTAINER_IMAGE override
            assert!(
                env.contains("DEVCONTAINER_IMAGE"),
                "{stack:?} env.sample missing DEVCONTAINER_IMAGE documentation: {env}"
            );
            // All stacks should document DEVCONTAINER_PULL_POLICY
            assert!(
                env.contains("DEVCONTAINER_PULL_POLICY"),
                "{stack:?} env.sample missing DEVCONTAINER_PULL_POLICY documentation: {env}"
            );
        }
    }

    #[test]
    fn rails_dockerfile_uses_microsoft_devcontainer_base() {
        let dockerfile = dockerfile(Stack::Rails).expect("rails dockerfile");
        // Rails should use Microsoft's official devcontainer Ruby image
        assert!(
            dockerfile.contains("mcr.microsoft.com/devcontainers/ruby"),
            "Rails Dockerfile should use mcr.microsoft.com/devcontainers/ruby base: {dockerfile}"
        );
        // Should NOT use non-existent ghcr.io/rails image
        assert!(
            !dockerfile.contains("ghcr.io/rails/devcontainer"),
            "Rails Dockerfile should not reference non-existent ghcr.io/rails image: {dockerfile}"
        );
    }

    #[test]
    fn all_stacks_have_privileged_mode_for_docker_in_docker() {
        for stack in [Stack::Rust, Stack::Rails, Stack::NodeJs, Stack::Generic] {
            let compose = compose_yaml(stack).expect("compose template");
            // All stacks need privileged mode for Docker-in-Docker
            assert!(
                compose.contains("privileged: true"),
                "{stack:?} compose template missing 'privileged: true': {compose}"
            );
        }
    }

    #[test]
    fn compose_templates_have_consistent_image_pattern() {
        let expected_patterns = [
            (
                Stack::Rust,
                "ghcr.io/branchbox/branchbox/devcontainer-rust:latest",
            ),
            (
                Stack::Rails,
                "ghcr.io/branchbox/branchbox/devcontainer-rails:latest",
            ),
            (
                Stack::NodeJs,
                "ghcr.io/branchbox/branchbox/devcontainer-nodejs:latest",
            ),
            (
                Stack::Generic,
                "ghcr.io/branchbox/branchbox/devcontainer-generic:latest",
            ),
        ];

        for (stack, expected_image) in expected_patterns {
            let compose = compose_yaml(stack).expect("compose template");
            assert!(
                compose.contains(expected_image),
                "{stack:?} compose template should reference {expected_image}: {compose}"
            );
        }
    }
}
