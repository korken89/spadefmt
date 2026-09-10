// Copyright (C) 2025 Ethan Uppal.
//
// This file is part of spadefmt.
//
// spadefmt is free software: you can redistribute it and/or modify it under the
// terms of the GNU General Public License as published by the Free Software
// Foundation, version 3 of the License only. spadefmt is distributed in the
// hope that it will be useful, but WITHOUT ANY WARRANTY; without even the
// implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See
// the GNU General Public License for more details. You should have received a
// copy of the GNU General Public License along with spadefmt. If not, see
// <https://www.gnu.org/licenses/>.

//! End-to-end tests of the `spadefmt` binary: config discovery, output
//! modes, and exit codes. Each test runs in its own scratch tree under the
//! system temp directory rather than `CARGO_TARGET_TMPDIR`, which sits
//! inside the repo: config discovery would find the repo's own
//! `spadefmt.toml` from any tree below it.

use std::{
    env, fs,
    io::Write,
    ops::Deref,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, SystemTime},
};

const UNFORMATTED: &str =
    "fn add(a: int<8>,   b: int<8>) -> int<8> {\n  a + b\n}\n";
const FORMATTED: &str =
    "fn add(a: int<8>, b: int<8>) -> int<8> {\n    a + b\n}\n";

/// Formatted at the default width; its head line is wider than 80 columns.
const WIDE: &str = "fn wide(alpha: int<8>, bravo: int<8>, charlie: int<8>, \
                    delta: int<8>, echo: int<8>) -> int<8> {\n    alpha\n}\n";
/// `WIDE` at `max_width = 40`.
const WIDE_BROKEN: &str = "fn wide(\n    alpha: int<8>,\n    bravo: int<8>,\n    \
                           charlie: int<8>,\n    delta: int<8>,\n    echo: \
                           int<8>,\n) -> int<8> {\n    alpha\n}\n";

const PARSE_ERROR: &str = "fn broken( {\n";

fn unsupported_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("asts/unsupported/multiple.spade")
}

/// A fresh, empty scratch tree for one test, removed on drop.
struct Scratch(PathBuf);

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// No ancestor of the scratch tree may hold a `spadefmt.toml`, or discovery
/// would pick it up in every test.
fn scratch(name: &str) -> Scratch {
    let dir = env::temp_dir().join("spadefmt-cli-tests").join(name);
    let _ = fs::remove_dir_all(&dir);
    for ancestor in dir.ancestors().skip(1) {
        let stray = ancestor.join("spadefmt.toml");
        assert!(
            !stray.exists(),
            "{} would be discovered by every CLI test; remove it",
            stray.display()
        );
    }
    fs::create_dir_all(&dir).expect("scratch directory should be creatable");
    Scratch(dir)
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent should be creatable");
    }
    fs::write(path, contents)
        .unwrap_or_else(|error| panic!("failed to write {path:?}: {error}"));
}

fn set_mtime(path: &Path, time: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(time))
        .unwrap_or_else(|error| panic!("failed to touch {path:?}: {error}"));
}

fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or_else(|error| panic!("failed to stat {path:?}: {error}"))
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl From<Output> for Run {
    fn from(output: Output) -> Self {
        Self {
            code: output.status.code().expect("spadefmt should exit normally"),
            stdout: String::from_utf8(output.stdout).expect("stdout is UTF-8"),
            stderr: String::from_utf8(output.stderr).expect("stderr is UTF-8"),
        }
    }
}

fn command(cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_spadefmt"));
    command
        .args(args)
        .current_dir(cwd)
        .env_remove("NO_COLOR")
        .stdin(Stdio::null());
    command
}

fn run(command: &mut Command) -> Run {
    command.output().expect("spadefmt should start").into()
}

fn spadefmt(cwd: &Path, args: &[&str]) -> Run {
    run(&mut command(cwd, args))
}

fn spadefmt_stdin(cwd: &Path, args: &[&str], input: &str) -> Run {
    let mut child = command(cwd, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spadefmt should start");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input.as_bytes())
        .expect("stdin should accept the input");
    child
        .wait_with_output()
        .expect("spadefmt should exit")
        .into()
}

#[test]
fn defaults_without_config() {
    let head = WIDE.lines().next().unwrap();
    assert!(head.len() > 80 && head.len() <= 100, "{head}");
    let dir = scratch("defaults_without_config");
    write(&dir.join("wide.spade"), WIDE);
    let run = spadefmt(&dir, &["wide.spade"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, WIDE);
    assert_eq!(run.stderr, "");
}

#[test]
fn discovers_the_nearest_config_from_the_file() {
    let dir = scratch("discovers_the_nearest_config_from_the_file");
    write(&dir.join("spadefmt.toml"), "max_width = 200\n");
    write(&dir.join("a/spadefmt.toml"), "max_width = 40\n");
    write(&dir.join("a/b/wide.spade"), WIDE);
    let run = spadefmt(&dir, &["a/b/wide.spade"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, WIDE_BROKEN);
}

#[test]
fn config_flag_overrides_discovery() {
    let dir = scratch("config_flag_overrides_discovery");
    write(&dir.join("spadefmt.toml"), "max_width = 40\n");
    write(&dir.join("wide.toml"), "max_width = 200\n");
    write(&dir.join("wide.spade"), WIDE);
    let run = spadefmt(&dir, &["--config", "wide.toml", "wide.spade"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, WIDE);
}

#[test]
fn unknown_config_key_is_an_error() {
    let dir = scratch("unknown_config_key_is_an_error");
    write(&dir.join("spadefmt.toml"), "bogus = 1\n");
    write(&dir.join("ok.spade"), FORMATTED);
    let run = spadefmt(&dir, &["ok.spade"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr.starts_with("error: invalid config "),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("spadefmt.toml"), "{}", run.stderr);
    assert!(
        run.stderr.contains("unknown field `bogus`"),
        "{}",
        run.stderr
    );
}

#[test]
fn zero_max_width_is_an_error() {
    let dir = scratch("zero_max_width_is_an_error");
    write(&dir.join("spadefmt.toml"), "max_width = 0\n");
    write(&dir.join("ok.spade"), FORMATTED);
    let run = spadefmt(&dir, &["ok.spade"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr.contains("1 is the minimum allowed"),
        "{}",
        run.stderr
    );
}

#[test]
fn a_broken_config_is_reported_once() {
    let dir = scratch("a_broken_config_is_reported_once");
    write(&dir.join("spadefmt.toml"), "bogus = 1\n");
    write(&dir.join("a.spade"), FORMATTED);
    write(&dir.join("b.spade"), FORMATTED);
    let run = spadefmt(&dir, &["--check", "a.spade", "b.spade"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert_eq!(
        run.stderr.matches("invalid config").count(),
        1,
        "{}",
        run.stderr
    );
}

#[test]
fn formats_one_file_to_stdout() {
    let dir = scratch("formats_one_file_to_stdout");
    let file = dir.join("add.spade");
    write(&file, UNFORMATTED);
    let run = spadefmt(&dir, &["add.spade"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, FORMATTED);
    assert_eq!(fs::read_to_string(&file).unwrap(), UNFORMATTED);
}

#[test]
fn several_inputs_need_a_mode() {
    let dir = scratch("several_inputs_need_a_mode");
    write(&dir.join("a.spade"), FORMATTED);
    write(&dir.join("b.spade"), FORMATTED);
    let run = spadefmt(&dir, &["a.spade", "b.spade"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(run.stderr.starts_with("error: "), "{}", run.stderr);
    assert!(run.stderr.contains("--check"), "{}", run.stderr);
}

#[test]
fn a_directory_needs_a_mode() {
    let dir = scratch("a_directory_needs_a_mode");
    write(&dir.join("src/a.spade"), FORMATTED);
    let run = spadefmt(&dir, &["src"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr.starts_with("error: src is a directory"),
        "{}",
        run.stderr
    );
}

#[test]
fn check_and_in_place_are_exclusive() {
    let dir = scratch("check_and_in_place_are_exclusive");
    write(&dir.join("a.spade"), FORMATTED);
    let run = spadefmt(&dir, &["--check", "--in-place", "a.spade"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert!(run.stderr.contains("mutually exclusive"), "{}", run.stderr);
}

#[test]
fn check_is_silent_on_formatted_files() {
    let dir = scratch("check_is_silent_on_formatted_files");
    write(&dir.join("a.spade"), FORMATTED);
    let run = spadefmt(&dir, &["--check", "a.spade"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert_eq!(run.stderr, "");
}

#[test]
fn check_lists_files_that_would_change() {
    let dir = scratch("check_lists_files_that_would_change");
    write(&dir.join("good.spade"), FORMATTED);
    write(&dir.join("bad.spade"), UNFORMATTED);
    let run = spadefmt(&dir, &["--check", "good.spade", "bad.spade"]);
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(run.stdout, "would reformat: bad.spade\n");
    assert_eq!(run.stderr, "");
    assert_eq!(
        fs::read_to_string(dir.join("bad.spade")).unwrap(),
        UNFORMATTED
    );
}

#[test]
fn in_place_rewrites_only_changed_files() {
    let dir = scratch("in_place_rewrites_only_changed_files");
    let good = dir.join("good.spade");
    let bad = dir.join("bad.spade");
    write(&good, FORMATTED);
    write(&bad, UNFORMATTED);
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    set_mtime(&good, old);
    set_mtime(&bad, old);

    let run = spadefmt(&dir, &["-i", "good.spade", "bad.spade"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert_eq!(fs::read_to_string(&bad).unwrap(), FORMATTED);
    assert!(mtime(&bad) > old, "bad.spade was not rewritten");
    assert_eq!(fs::read_to_string(&good).unwrap(), FORMATTED);
    assert_eq!(mtime(&good), old, "good.spade was rewritten unchanged");
}

#[test]
fn directories_recurse_for_spade_files() {
    let dir = scratch("directories_recurse_for_spade_files");
    write(&dir.join("src/a.spade"), UNFORMATTED);
    write(&dir.join("src/nested/c.spade"), FORMATTED);
    write(&dir.join("src/nested/deep/b.spade"), UNFORMATTED);
    write(&dir.join("src/notes.txt"), UNFORMATTED);
    write(&dir.join("src/nested/README"), PARSE_ERROR);
    let run = spadefmt(&dir, &["--check", "src"]);
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "would reformat: src/a.spade\nwould reformat: src/nested/deep/b.spade\n"
    );
    assert_eq!(run.stderr, "");
}

#[cfg(unix)]
#[test]
fn symlinked_directories_are_not_followed() {
    use std::os::unix::fs::symlink;

    let dir = scratch("symlinked_directories_are_not_followed");
    write(&dir.join("cyc/a/bad.spade"), UNFORMATTED);
    write(&dir.join("elsewhere.spade"), UNFORMATTED);
    symlink("..", dir.join("cyc/a/up")).expect("symlink should be creatable");
    symlink("../elsewhere.spade", dir.join("cyc/link.spade"))
        .expect("symlink should be creatable");
    let run = spadefmt(&dir, &["--check", "cyc"]);
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "would reformat: cyc/a/bad.spade\nwould reformat: cyc/link.spade\n"
    );
    assert_eq!(run.stderr, "");
    let run = spadefmt(&dir, &["-i", "cyc"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert_eq!(
        fs::read_to_string(dir.join("elsewhere.spade")).unwrap(),
        FORMATTED
    );
}

#[test]
fn a_closed_stdout_ends_the_run_quietly() {
    let dir = scratch("a_closed_stdout_ends_the_run_quietly");
    // Well past any pipe buffer, so the write blocks until the reader is
    // gone.
    write(&dir.join("big.spade"), &UNFORMATTED.repeat(4000));
    let mut child = command(&dir, &["big.spade"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spadefmt should start");
    drop(child.stdout.take());
    let run: Run = child
        .wait_with_output()
        .expect("spadefmt should exit")
        .into();
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stderr, "");
}

#[test]
fn stdin_keeps_its_position_among_the_inputs() {
    let dir = scratch("stdin_keeps_its_position_among_the_inputs");
    write(&dir.join("bad.spade"), UNFORMATTED);
    let run = spadefmt_stdin(&dir, &["--check", "-", "bad.spade"], UNFORMATTED);
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "would reformat: <stdin>\nwould reformat: bad.spade\n"
    );
    let run = spadefmt_stdin(&dir, &["--check", "bad.spade", "-"], UNFORMATTED);
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "would reformat: bad.spade\nwould reformat: <stdin>\n"
    );
}

#[test]
fn a_dash_config_is_a_path_not_stdin() {
    let dir = scratch("a_dash_config_is_a_path_not_stdin");
    write(&dir.join("ok.spade"), FORMATTED);
    let run = spadefmt(&dir, &["--config", "-", "ok.spade"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr.starts_with("error: failed to read config -\n"),
        "{}",
        run.stderr
    );
}

#[test]
fn an_empty_file_stays_empty() {
    let dir = scratch("an_empty_file_stays_empty");
    let empty = dir.join("empty.spade");
    let blank = dir.join("blank.spade");
    write(&empty, "");
    write(&blank, " \n\n");
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    set_mtime(&empty, old);
    let run = spadefmt(&dir, &["--check", "empty.spade"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    let run = spadefmt(&dir, &["-i", "empty.spade", "blank.spade"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(mtime(&empty), old, "empty.spade was rewritten");
    assert_eq!(fs::read_to_string(&blank).unwrap(), "");
}

#[test]
fn stdin_formats_to_stdout() {
    let dir = scratch("stdin_formats_to_stdout");
    let run = spadefmt_stdin(&dir, &["-"], UNFORMATTED);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, FORMATTED);
    assert!(
        fs::read_dir(&dir).unwrap().next().is_none(),
        "stdin wrote a file"
    );
}

#[test]
fn stdin_check() {
    let dir = scratch("stdin_check");
    let run = spadefmt_stdin(&dir, &["--check", "-"], UNFORMATTED);
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert_eq!(run.stdout, "would reformat: <stdin>\n");
    let run = spadefmt_stdin(&dir, &["--check", "-"], FORMATTED);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, "");
}

#[test]
fn stdin_discovers_config_from_the_working_directory() {
    let dir = scratch("stdin_discovers_config_from_the_working_directory");
    write(&dir.join("spadefmt.toml"), "max_width = 40\n");
    let run = spadefmt_stdin(&dir, &["-"], WIDE);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, WIDE_BROKEN);
}

#[test]
fn parse_error_exits_3() {
    let dir = scratch("parse_error_exits_3");
    write(&dir.join("broken.spade"), PARSE_ERROR);
    let run = spadefmt(&dir, &["broken.spade"]);
    assert_eq!(run.code, 3, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(run.stderr.starts_with("error"), "{}", run.stderr);
    assert!(run.stderr.contains("broken.spade:1:"), "{}", run.stderr);
}

#[test]
fn unsupported_construct_exits_4() {
    let dir = scratch("unsupported_construct_exits_4");
    let fixture = unsupported_fixture();
    let run = spadefmt(&dir, &["--check", fixture.to_str().unwrap()]);
    assert_eq!(run.code, 4, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr
            .matches("Expected an identifier after `.`")
            .count(),
        2,
        "{}",
        run.stderr
    );
}

#[test]
fn missing_file_exits_2() {
    let dir = scratch("missing_file_exits_2");
    let run = spadefmt(&dir, &["missing.spade"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr
            .starts_with("error: failed to read missing.spade\ncaused by: "),
        "{}",
        run.stderr
    );
}

#[test]
fn mixed_inputs_exit_with_the_worst_outcome() {
    let dir = scratch("mixed_inputs_exit_with_the_worst_outcome");
    write(&dir.join("bad.spade"), UNFORMATTED);
    write(&dir.join("broken.spade"), PARSE_ERROR);
    let fixture = unsupported_fixture();
    let run = spadefmt(
        &dir,
        &[
            "--check",
            "bad.spade",
            "broken.spade",
            "missing.spade",
            fixture.to_str().unwrap(),
        ],
    );
    assert_eq!(run.code, 4, "{}", run.stderr);
    assert_eq!(run.stdout, "would reformat: bad.spade\n");
    for expected in [
        "broken.spade:1:",
        "error: failed to read missing.spade",
        "Expected an identifier after `.`",
    ] {
        assert!(
            run.stderr.contains(expected),
            "missing {expected:?}:\n{}",
            run.stderr
        );
    }
    assert_eq!(
        fs::read_to_string(dir.join("bad.spade")).unwrap(),
        UNFORMATTED
    );
}

#[test]
fn no_color_accepts_any_value() {
    let dir = scratch("no_color_accepts_any_value");
    write(&dir.join("broken.spade"), PARSE_ERROR);
    let run = run(command(&dir, &["broken.spade"]).env("NO_COLOR", "anything"));
    assert_eq!(run.code, 3, "{}", run.stderr);
    assert!(!run.stderr.contains('\x1b'), "{}", run.stderr);
}

#[test]
fn debug_prints_the_document() {
    let dir = scratch("debug_prints_the_document");
    write(&dir.join("add.spade"), UNFORMATTED);
    let run = spadefmt(&dir, &["--debug", "add.spade"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(
        !run.stdout.is_empty() && run.stdout != FORMATTED,
        "{}",
        run.stdout
    );
    let run = spadefmt(&dir, &["--debug", "--check", "add.spade"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
}

#[test]
fn no_inputs_is_a_usage_error() {
    let dir = scratch("no_inputs_is_a_usage_error");
    let run = spadefmt(&dir, &[]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr.starts_with("error: no input files\n"),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("Run spadefmt --help"), "{}", run.stderr);
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let dir = scratch("unknown_flag_is_a_usage_error");
    let run = spadefmt(&dir, &["--bogus"]);
    assert_eq!(run.code, 2, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    assert!(run.stderr.starts_with("error: "), "{}", run.stderr);
    assert!(run.stderr.contains("--bogus"), "{}", run.stderr);
    assert!(run.stderr.contains("Run spadefmt --help"), "{}", run.stderr);
}

#[test]
fn help_documents_the_positional() {
    let dir = scratch("help_documents_the_positional");
    let run = spadefmt(&dir, &["--help"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stderr, "");
    for expected in [
        "[<files...>]",
        "files or directories to format",
        "--check",
        "--in-place",
        "--config <path>",
    ] {
        assert!(
            run.stdout.contains(expected),
            "missing {expected:?}:\n{}",
            run.stdout
        );
    }
}

#[test]
fn version_exits_0() {
    let dir = scratch("version_exits_0");
    for flag in ["-v", "--version"] {
        let run = spadefmt(&dir, &[flag]);
        assert_eq!(run.code, 0, "{}", run.stderr);
        assert!(
            run.stdout.contains(env!("CARGO_PKG_VERSION")),
            "{}",
            run.stdout
        );
    }
}
