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
