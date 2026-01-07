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
}
