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
        // SSH keys mount (read-only for security)
        assert!(
            compose.contains("~/.ssh:/home/vscode/.ssh:ro"),
            "compose template missing SSH keys mount: {compose}"
        );
        // AI agent configs are encapsulated under .ai-agents/ directory
        for shared in [
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/codex:/home/vscode/.codex",
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/claude:/home/vscode/.claude",
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/claude.json:/home/vscode/.claude.json",
            "${SHARED_CONFIG_DIR:-../..}/.ai-agents/gh:/home/vscode/.config/gh",
        ] {
            assert!(
                compose.contains(shared),
                "compose template missing shared config volume {shared}: {compose}"
            );
        }
    }

    #[test]
    fn devcontainer_template_includes_ssh_agent_forwarding() {
        for stack in [Stack::Rust, Stack::Rails, Stack::NodeJs, Stack::Generic] {
            let config = devcontainer_json(stack).expect("devcontainer template");
            assert!(
                config.contains(r#""SSH_AUTH_SOCK": "${localEnv:SSH_AUTH_SOCK}""#),
                "devcontainer template for {:?} missing SSH agent forwarding: {config}",
                stack
            );
        }
    }

    #[test]
    fn compose_template_includes_ssh_mount_for_all_stacks() {
        for stack in [Stack::Rust, Stack::Rails, Stack::NodeJs, Stack::Generic] {
            let compose = compose_yaml(stack).expect("compose template");
            assert!(
                compose.contains("~/.ssh:/home/vscode/.ssh:ro"),
                "compose template for {:?} missing SSH keys mount: {compose}",
                stack
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
    fn rails_dockerfile_uses_mise_for_ruby_version_management() {
        let dockerfile = dockerfile(Stack::Rails).expect("rails dockerfile");
        // Rails should use Microsoft's official devcontainer base image
        assert!(
            dockerfile.contains("mcr.microsoft.com/devcontainers/base:debian"),
            "Rails Dockerfile should use mcr.microsoft.com/devcontainers/base:debian: {dockerfile}"
        );
        // Should install mise for Ruby version management
        assert!(
            dockerfile.contains("mise.run"),
            "Rails Dockerfile should install mise: {dockerfile}"
        );
        // Should pre-install common Ruby versions
        assert!(
            dockerfile.contains("mise install ruby@3.3")
                || dockerfile.contains("mise install ruby"),
            "Rails Dockerfile should pre-install Ruby: {dockerfile}"
        );
        // Should NOT use non-existent ghcr.io/rails image
        assert!(
            !dockerfile.contains("ghcr.io/rails/devcontainer"),
            "Rails Dockerfile should not reference non-existent ghcr.io/rails image: {dockerfile}"
        );
    }

    #[test]
    fn rails_dockerfile_includes_common_dependencies() {
        let dockerfile = dockerfile(Stack::Rails).expect("rails dockerfile");
        // Should include image processing libraries
        assert!(
            dockerfile.contains("imagemagick"),
            "Rails Dockerfile should include imagemagick: {dockerfile}"
        );
        assert!(
            dockerfile.contains("libvips"),
            "Rails Dockerfile should include libvips: {dockerfile}"
        );
        // Should include SQLite support
        assert!(
            dockerfile.contains("sqlite3"),
            "Rails Dockerfile should include sqlite3: {dockerfile}"
        );
        // Should include gum for modern Rails bin/setup scripts
        assert!(
            dockerfile.contains("gum"),
            "Rails Dockerfile should include gum: {dockerfile}"
        );
    }

    #[test]
    fn rails_compose_includes_mise_cache_volume() {
        let compose = compose_yaml(Stack::Rails).expect("rails compose template");
        assert!(
            compose.contains("mise-cache:/home/vscode/.local/share/mise"),
            "Rails compose should include mise cache volume: {compose}"
        );
    }

    #[test]
    fn rails_compose_has_solid_queue_env_var() {
        let compose = compose_yaml(Stack::Rails).expect("rails compose template");
        assert!(
            compose.contains("SOLID_QUEUE_IN_PUMA"),
            "Rails compose should include SOLID_QUEUE_IN_PUMA env var: {compose}"
        );
    }

    #[test]
    fn nodejs_dockerfile_uses_mise_for_node_version_management() {
        let dockerfile = dockerfile(Stack::NodeJs).expect("nodejs dockerfile");
        // Node.js should use Microsoft's official devcontainer base image
        assert!(
            dockerfile.contains("mcr.microsoft.com/devcontainers/base:debian"),
            "Node.js Dockerfile should use mcr.microsoft.com/devcontainers/base:debian: {dockerfile}"
        );
        // Should install mise for Node version management
        assert!(
            dockerfile.contains("mise.run"),
            "Node.js Dockerfile should install mise: {dockerfile}"
        );
        // Should pre-install common Node versions
        assert!(
            dockerfile.contains("mise install node@20") || dockerfile.contains("mise install node"),
            "Node.js Dockerfile should pre-install Node: {dockerfile}"
        );
    }

    #[test]
    fn nodejs_dockerfile_includes_native_module_dependencies() {
        let dockerfile = dockerfile(Stack::NodeJs).expect("nodejs dockerfile");
        // Should include python for node-gyp
        assert!(
            dockerfile.contains("python3"),
            "Node.js Dockerfile should include python3 for node-gyp: {dockerfile}"
        );
        // Should include build-essential for native modules
        assert!(
            dockerfile.contains("build-essential"),
            "Node.js Dockerfile should include build-essential: {dockerfile}"
        );
        // Should include libvips for sharp image processing
        assert!(
            dockerfile.contains("libvips"),
            "Node.js Dockerfile should include libvips: {dockerfile}"
        );
    }

    #[test]
    fn nodejs_compose_includes_mise_and_npm_cache_volumes() {
        let compose = compose_yaml(Stack::NodeJs).expect("nodejs compose template");
        assert!(
            compose.contains("mise-cache:/home/vscode/.local/share/mise"),
            "Node.js compose should include mise cache volume: {compose}"
        );
        assert!(
            compose.contains("npm-cache:/home/vscode/.npm"),
            "Node.js compose should include npm cache volume: {compose}"
        );
    }

    #[test]
    fn nodejs_devcontainer_uses_mise_exec() {
        let devcontainer = devcontainer_json(Stack::NodeJs).expect("nodejs devcontainer.json");
        assert!(
            devcontainer.contains("mise exec -- npm install"),
            "Node.js devcontainer should use mise exec for npm install: {devcontainer}"
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
