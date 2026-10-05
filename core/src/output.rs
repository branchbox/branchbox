//! Process-wide output routing
//!
//! BranchBox has two audiences for what it prints: people reading a terminal and programs
//! (the macOS app, scripts) parsing `--json` output. Machine mode keeps them apart
//! (DESIGN §5.2):
//!
//! - stdout carries exactly one JSON document, written by [`emit_json`];
//! - human text written through [`humanln!`](crate::humanln) / [`human!`](crate::human) goes
//!   to stdout in text mode and to stderr in machine mode;
//! - nothing prompts ([`is_interactive`] is `false`).
//!
//! The CLI enables machine mode once, right after argument parsing, whenever a `--json` flag is
//! present. `println!`/`print!` are disallowed in `core` and `cli` (see their `clippy.toml`)
//! so no code path can write human text onto a JSON stdout by accident.

use crate::Result;
use serde::Serialize;
use std::fmt;
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

static MACHINE_MODE: AtomicBool = AtomicBool::new(false);
static DOCUMENT_EMITTED: AtomicBool = AtomicBool::new(false);

/// Switch the process into (or out of) machine mode.
pub fn set_machine_mode(enabled: bool) {
    MACHINE_MODE.store(enabled, Ordering::SeqCst);
}

/// Whether the process is in machine (`--json`) mode.
pub fn machine_mode() -> bool {
    MACHINE_MODE.load(Ordering::SeqCst)
}

/// Whether it is acceptable to prompt: not in machine mode, and both stdin and stdout are
/// terminals. Every prompt gate uses this so `--json` always takes the documented
/// non-interactive outcome instead of waiting on input nobody will type.
pub fn is_interactive() -> bool {
    !machine_mode() && io::stdin().is_terminal() && io::stdout().is_terminal()
}

/// Whether [`emit_json`] has written this process's JSON document. The CLI's failure path uses
/// it to decide whether an error envelope may still be printed on stdout.
pub fn document_emitted() -> bool {
    DOCUMENT_EMITTED.load(Ordering::SeqCst)
}

/// Write human-readable text: stdout in text mode, stderr in machine mode.
///
/// Prefer the [`humanln!`](crate::humanln) and [`human!`](crate::human) macros. A closed pipe
/// (`branchbox … | head`) is ignored rather than turned into a panic as `println!` would.
pub fn write_human(args: fmt::Arguments<'_>) {
    let result = if machine_mode() {
        write_human_to(&mut io::stderr().lock(), args)
    } else {
        write_human_to(&mut io::stdout().lock(), args)
    };
    log_human_write_failure(result);
}

/// Human text is best effort: a closed pipe is expected (`| head`) and anything else is only
/// logged, so printing can never fail a command.
fn log_human_write_failure(result: io::Result<()>) {
    if let Err(err) = result {
        if err.kind() != io::ErrorKind::BrokenPipe {
            tracing::debug!("Failed to write human output: {err}");
        }
    }
}

fn write_human_to(writer: &mut impl Write, args: fmt::Arguments<'_>) -> io::Result<()> {
    writer.write_fmt(args)?;
    // `print!` callers rely on an explicit flush before reading a reply; stderr is unbuffered
    // and stdout is line-buffered, so flushing here keeps prompts visible in both modes.
    writer.flush()
}

/// Print `value` as this process's single JSON document on stdout: pretty-printed, newline
/// terminated and flushed, byte-for-byte what `println!("{}", to_string_pretty(value)?)` wrote.
///
/// The value is serialized before anything is written, so a serialization error leaves stdout
/// untouched and the caller's error envelope can still be printed. A closed pipe is not an error.
/// Debug builds assert that no second document is ever emitted.
pub fn emit_json<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    let document = render_json(value)?;
    let already_emitted = DOCUMENT_EMITTED.swap(true, Ordering::SeqCst);
    debug_assert!(
        !already_emitted,
        "emit_json called twice: stdout must carry exactly one JSON document"
    );
    document_write_result(write_document(&mut io::stdout().lock(), &document))
}

/// A reader that closed stdout early (`| head`) is not an error; any other write failure is.
fn document_write_result(result: io::Result<()>) -> Result<()> {
    match result {
        Err(err) if err.kind() != io::ErrorKind::BrokenPipe => Err(err.into()),
        _ => Ok(()),
    }
}

fn render_json<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>> {
    let mut document = serde_json::to_vec_pretty(value)?;
    document.push(b'\n');
    Ok(document)
}

fn write_document(writer: &mut impl Write, document: &[u8]) -> io::Result<()> {
    writer.write_all(document)?;
    writer.flush()
}

/// Print a line of human-readable text (stdout in text mode, stderr in `--json` mode).
///
/// Drop-in replacement for `println!`, which `core` and `cli` disallow.
#[macro_export]
macro_rules! humanln {
    () => {
        $crate::output::write_human(::std::format_args!("\n"))
    };
    ($($arg:tt)*) => {
        $crate::output::write_human(::std::format_args!("{}\n", ::std::format_args!($($arg)*)))
    };
}

/// Print human-readable text without a newline (stdout in text mode, stderr in `--json` mode).
///
/// Drop-in replacement for `print!`, which `core` and `cli` disallow. The text is flushed, so a
/// prompt is visible before the caller reads the reply.
#[macro_export]
macro_rules! human {
    ($($arg:tt)*) => {
        $crate::output::write_human(::std::format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes the tests that flip the process-wide flags.
    static GLOBAL_STATE: Mutex<()> = Mutex::new(());

    struct BrokenPipe;

    impl Write for BrokenPipe {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn machine_mode_round_trips_and_disables_prompts() {
        let _guard = GLOBAL_STATE.lock().unwrap_or_else(|err| err.into_inner());
        set_machine_mode(true);
        assert!(machine_mode());
        assert!(!is_interactive(), "machine mode never prompts");
        set_machine_mode(false);
        assert!(!machine_mode());
    }

    #[test]
    fn is_interactive_requires_terminals() {
        let _guard = GLOBAL_STATE.lock().unwrap_or_else(|err| err.into_inner());
        set_machine_mode(false);
        assert_eq!(
            is_interactive(),
            io::stdin().is_terminal() && io::stdout().is_terminal()
        );
    }

    #[test]
    fn human_text_is_written_verbatim_and_flushed() {
        let mut buffer = Vec::new();
        write_human_to(&mut buffer, format_args!("{}-{}\n", "a", 1)).unwrap();
        assert_eq!(buffer, b"a-1\n");
    }

    #[test]
    fn write_human_survives_both_modes() {
        let _guard = GLOBAL_STATE.lock().unwrap_or_else(|err| err.into_inner());
        set_machine_mode(true);
        crate::humanln!();
        crate::human!("{}", "");
        set_machine_mode(false);
        crate::human!("");
        crate::humanln!("{}", "");
    }

    #[test]
    fn json_document_matches_legacy_println_bytes() {
        let value = serde_json::json!({"work_feature": "eta", "warnings": ["w"], "n": 1});
        let legacy = format!("{}\n", serde_json::to_string_pretty(&value).unwrap());
        assert_eq!(render_json(&value).unwrap(), legacy.into_bytes());

        let mut buffer = Vec::new();
        write_document(&mut buffer, b"{}\n").unwrap();
        assert_eq!(buffer, b"{}\n");
    }

    #[test]
    fn serialization_failure_writes_nothing() {
        // Maps with non-string keys cannot be represented in JSON.
        let mut value = std::collections::BTreeMap::new();
        value.insert(vec![1u8], "x");
        assert!(render_json(&value).is_err());
    }

    #[test]
    fn broken_pipe_is_reported_by_the_writer_helpers() {
        assert_eq!(
            write_document(&mut BrokenPipe, b"{}").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            write_human_to(&mut BrokenPipe, format_args!("x"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn write_failures_other_than_a_closed_pipe() {
        // Human text never fails the command, whatever went wrong.
        log_human_write_failure(Ok(()));
        log_human_write_failure(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
        log_human_write_failure(Err(io::Error::other("disk full")));

        // The JSON document only tolerates a closed pipe.
        assert!(document_write_result(Ok(())).is_ok());
        assert!(document_write_result(Err(io::Error::from(io::ErrorKind::BrokenPipe))).is_ok());
        let err = document_write_result(Err(io::Error::other("disk full"))).unwrap_err();
        assert!(err.to_string().contains("disk full"), "{err}");
    }

    #[test]
    fn emit_json_marks_the_document_as_emitted() {
        let _guard = GLOBAL_STATE.lock().unwrap_or_else(|err| err.into_inner());
        DOCUMENT_EMITTED.store(false, Ordering::SeqCst);
        assert!(!document_emitted());
        emit_json(&serde_json::json!({"schema_version": 1})).unwrap();
        assert!(document_emitted());
        DOCUMENT_EMITTED.store(false, Ordering::SeqCst);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn second_document_trips_the_debug_assertion() {
        let _guard = GLOBAL_STATE.lock().unwrap_or_else(|err| err.into_inner());
        DOCUMENT_EMITTED.store(true, Ordering::SeqCst);
        let result = std::panic::catch_unwind(|| emit_json(&serde_json::json!({})));
        DOCUMENT_EMITTED.store(false, Ordering::SeqCst);
        assert!(
            result.is_err(),
            "a second document must panic in debug builds"
        );
    }
}
