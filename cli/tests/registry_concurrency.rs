//! Concurrent registry writers (DESIGN §10.1, S2).
//!
//! Every `branchbox` process that changes `.branchbox/registry.json` takes the state-directory
//! lock for its read-modify-write cycle, and every `git worktree` change takes the repository's
//! worktree lock. These tests release a dozen starts or teardowns at once and check, end to end,
//! that every process succeeds and that no registry update is lost. (The deterministic
//! lost-update proof, with a widened read-modify-write window, is a core unit test.)
#![cfg(unix)]
#![cfg_attr(test, allow(clippy::disallowed_macros))]

#[macro_use]
mod support;

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Output;
use std::sync::Barrier;
use std::thread;
use support::init_test_repo;

const WRITERS: usize = 12;

fn feature_names(prefix: &str) -> Vec<String> {
    (0..WRITERS)
        .map(|index| format!("{prefix}-{index}"))
        .collect()
}

/// Run one command per name, all released at the same moment, and return their outputs.
fn run_concurrently(names: &[String], command: impl Fn(&str) -> Command) -> Vec<Output> {
    let barrier = Barrier::new(names.len());
    thread::scope(|scope| {
        let handles: Vec<_> = names
            .iter()
            .map(|name| {
                let mut cmd = command(name);
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    cmd.output().expect("spawn branchbox")
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("writer thread panicked"))
            .collect()
    })
}

fn assert_all_succeeded(names: &[String], outputs: &[Output]) {
    for (name, output) in names.iter().zip(outputs) {
        assert!(
            output.status.success(),
            "{name} failed ({}):\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        support::assert_single_json(output);
    }
}

/// `work_feature` → `status` for every registry entry, plus whether any entry kept a setup
/// marker.
fn registry_statuses(repo: &Path) -> (Vec<(String, String)>, bool) {
    let registry: Value = serde_json::from_slice(
        &fs::read(repo.join(".branchbox/registry.json")).expect("read registry"),
    )
    .expect("registry is valid JSON");
    let features = registry["features"].as_array().expect("features array");
    let mut statuses: Vec<(String, String)> = features
        .iter()
        .map(|feature| {
            (
                feature["work_feature"].as_str().unwrap().to_string(),
                feature["status"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    statuses.sort();
    let any_setup = features
        .iter()
        .any(|feature| feature.get("setup").is_some());
    (statuses, any_setup)
}

fn expected(names: &[String], status: &str) -> Vec<(String, String)> {
    let mut expected: Vec<(String, String)> = names
        .iter()
        .map(|name| (name.clone(), status.to_string()))
        .collect();
    expected.sort();
    expected
}

#[test]
fn concurrent_forced_teardowns_remove_every_entry() {
    let test_repo = init_test_repo();
    let repo = test_repo.path();
    let names = feature_names("down");
    for name in &names {
        let output = branchbox_cmd!(repo)
            .args(["feature", "start", name, "--minimal", "--json"])
            .output()
            .expect("run feature start");
        assert_all_succeeded(std::slice::from_ref(name), std::slice::from_ref(&output));
    }
    assert_eq!(registry_statuses(repo).0, expected(&names, "active"));

    let outputs = run_concurrently(&names, |name| {
        let mut cmd = branchbox_cmd!(repo);
        cmd.args(["feature", "teardown", name, "--force", "--json"]);
        cmd
    });
    assert_all_succeeded(&names, &outputs);

    assert_eq!(registry_statuses(repo).0, expected(&names, "removed"));
    for name in &names {
        assert!(
            !test_repo.root().join(name).exists(),
            "{name} worktree left behind"
        );
    }
    let listed = branchbox_cmd!(repo)
        .args(["feature", "list", "--json"])
        .output()
        .expect("run feature list");
    assert!(listed.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&listed.stdout).unwrap(),
        serde_json::json!([])
    );
}

#[test]
fn concurrent_minimal_starts_record_every_feature() {
    let test_repo = init_test_repo();
    let repo = test_repo.path();
    let names = feature_names("up");

    let outputs = run_concurrently(&names, |name| {
        let mut cmd = branchbox_cmd!(repo);
        cmd.args(["feature", "start", name, "--minimal", "--json"]);
        cmd
    });
    assert_all_succeeded(&names, &outputs);

    let (statuses, any_setup) = registry_statuses(repo);
    assert_eq!(statuses, expected(&names, "active"));
    assert!(
        !any_setup,
        "every completed start clears its write-ahead marker"
    );
    for name in &names {
        assert!(
            test_repo.root().join(name).is_dir(),
            "{name} worktree missing"
        );
    }
    let leftovers: Vec<String> = fs::read_dir(repo.join(".branchbox"))
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
}
