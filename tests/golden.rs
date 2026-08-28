// Copyright (C) 2024 Ethan Uppal.
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

//! Corpus tests over `asts/`.
//!
//! - Golden: `format(asts/X.spade)` must equal `asts/X-formatted.spade`; a
//!   golden whose input is gone is an orphan and fails. Regenerate the goldens
//!   (and delete orphans) with `SPADEFMT_BLESS=1 cargo test`.
//! - Idempotency: formatting must be a fixed point. [`KNOWN_NON_IDEMPOTENT`]
//!   lists inputs where it is not yet; entries are asserted to stay broken so
//!   fixes must shrink the list.
//! - Panic sweep: formatting any `.spade` file under `asts/` (recursively) must
//!   not panic. [`KNOWN_PANICS`] works like [`KNOWN_NON_IDEMPOTENT`].

use std::{
    env, fs, io,
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
};

use spadefmt::{
    config::Config,
    format::{FormatError, Formatted, format_source},
};

/// Inputs that `format` does not yet map to a fixed point. Steps fixing
/// comment attachment and blank-line preservation shrink this list.
const KNOWN_NON_IDEMPOTENT: &[&str] = &[
    "keepemptylines.spade",
    "rv.spade",
    "test.spade",
    "test4.spade",
];

/// Fixtures (relative to `asts/`) that hit a `todo!()` in the document
/// builder. Fixing a construct moves its fixture up into `asts/` as a golden
/// pair and removes it here. `swim-templates/` entries are only asserted when
/// the submodule is initialized.
const KNOWN_PANICS: &[&str] = &[
    "panics/named_arg_pattern.spade",
    "panics/pipeline_reg.spade",
    "panics/tuple_index.spade",
    "panics/unsafe_block.spade",
    "swim-templates/ccgm1a1-evb/src/main.spade",
    "swim-templates/ecpix5/src/main.spade",
    "swim-templates/fomu-pvt/src/main.spade",
    "swim-templates/go-board/src/main.spade",
    "swim-templates/icebreaker/src/main.spade",
    "swim-templates/tangnano20k/src/main.spade",
    "swim-templates/tangnano4k/src/main.spade",
    "swim-templates/tangnano9k/src/main.spade",
    "swim-templates/ulx3s_85k/src/main.spade",
];

const BLESS_HINT: &str =
    "regenerate goldens with `SPADEFMT_BLESS=1 cargo test`";

fn asts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("asts")
}

fn config() -> Config {
    toml::from_str(include_str!("../spadefmt.toml"))
        .expect("repo spadefmt.toml should parse")
}

fn bless_mode() -> bool {
    let Some(value) = env::var_os("SPADEFMT_BLESS") else {
        return false;
    };
    match value.to_str() {
        Some("1") | Some("true") => true,
        Some("") | Some("0") | Some("false") => false,
        _ => panic!(
            "unrecognized SPADEFMT_BLESS value {value:?}; set \
             SPADEFMT_BLESS=1 to regenerate goldens"
        ),
    }
}

fn is_golden(path: &Path) -> bool {
    path.file_stem()
        .is_some_and(|stem| stem.to_string_lossy().ends_with("-formatted"))
}

/// Every `asts/*.spade` (non-recursive), split into corpus inputs and
/// `-formatted` goldens.
fn top_level_spade_files() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut inputs = vec![];
    let mut goldens = vec![];
    for entry in fs::read_dir(asts_dir()).expect("asts/ should exist") {
        let path = entry.expect("asts/ should be readable").path();
        if path
            .extension()
            .is_none_or(|extension| extension != "spade")
        {
            continue;
        }
        if is_golden(&path) {
            goldens.push(path);
        } else {
            inputs.push(path);
        }
    }
    inputs.sort();
    goldens.sort();
    assert!(!inputs.is_empty(), "no corpus inputs found in asts/");
    (inputs, goldens)
}

fn corpus_inputs() -> Vec<PathBuf> {
    top_level_spade_files().0
}

/// Every `.spade` file under `asts/`, recursively.
fn all_spade_files(directory: &Path, into: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("directory should be readable")
    {
        let path = entry.expect("directory should be readable").path();
        if path.is_dir() {
            all_spade_files(&path, into);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "spade")
        {
            into.push(path);
        }
    }
}

/// Writes via a temporary file and rename, so the concurrently running panic
/// sweep never reads a half-written golden.
fn write_golden(path: &Path, contents: &str) {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents).unwrap_or_else(|error| {
        panic!("failed to write {temporary:?}: {error}")
    });
    fs::rename(&temporary, path).unwrap_or_else(|error| {
        panic!("failed to rename {temporary:?} to {path:?}: {error}")
    });
}

fn assert_clean_output(text: &str, source: &str) {
    assert!(
        text.ends_with('\n'),
        "output for {source} does not end with a newline"
    );
    // The same character set `format::into_clean_text` strips.
    for (index, line) in text.split('\n').enumerate() {
        assert!(
            !line.ends_with([' ', '\t', '\r']),
            "output for {source} has trailing whitespace on line {}",
            index + 1
        );
    }
}

fn format_str(code: &str, source: &str, config: &Config) -> String {
    let result = format_source(source, code, config, false);
    let Formatted { text, .. } = result.unwrap_or_else(|error| match error {
        FormatError::Parse { diagnostics } => {
            panic!("failed to parse {source}:\n{diagnostics}")
        }
        other => panic!("failed to format {source}: {other}"),
    });
    assert_clean_output(&text, source);
    text
}

fn format_file(path: &Path, config: &Config) -> String {
    let code = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {path:?}: {error}"));
    format_str(&code, &path.display().to_string(), config)
}

fn assert_text_eq(actual: &str, expected: &str, message: &str) {
    if actual == expected {
        return;
    }
    let line_count = |text: &str| text.lines().count();
    let difference_line = actual
        .lines()
        .zip(expected.lines())
        .position(|(actual_line, expected_line)| actual_line != expected_line)
        .map(|index| index + 1)
        .or_else(|| {
            (line_count(actual) != line_count(expected))
                .then(|| line_count(actual).min(line_count(expected)) + 1)
        });
    match difference_line {
        Some(line) => panic!(
            "{message}\nfirst difference at line {line}:\n  \
             actual:   {:?}\n  expected: {:?}",
            actual.lines().nth(line - 1).unwrap_or(""),
            expected.lines().nth(line - 1).unwrap_or("")
        ),
        // Line-wise equal but different as strings: line endings or
        // trailing newlines.
        None => {
            let offset = actual
                .bytes()
                .zip(expected.bytes())
                .position(|(actual_byte, expected_byte)| {
                    actual_byte != expected_byte
                })
                .unwrap_or(actual.len().min(expected.len()));
            panic!(
                "{message}\nlines are identical but the raw text differs \
                 (line endings or trailing newlines); first byte difference \
                 at offset {offset} (actual {} bytes, expected {} bytes)",
                actual.len(),
                expected.len()
            );
        }
    }
}

#[test]
fn golden() {
    let config = config();
    let bless = bless_mode();
    let (inputs, goldens) = top_level_spade_files();

    for input in &inputs {
        let formatted = format_file(input, &config);
        let stem = input.file_stem().unwrap().to_string_lossy();
        let golden_path =
            input.with_file_name(format!("{stem}-formatted.spade"));

        if bless {
            write_golden(&golden_path, &formatted);
        } else {
            let expected = match fs::read_to_string(&golden_path) {
                Ok(expected) => expected,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    panic!("missing golden {golden_path:?}; {BLESS_HINT}")
                }
                Err(error) => {
                    panic!("failed to read {golden_path:?}: {error}")
                }
            };
            assert_text_eq(
                &formatted,
                &expected,
                &format!(
                    "{} does not match its golden; if the new output is \
                     intended, {BLESS_HINT}",
                    input.display()
                ),
            );
        }
    }

    // A golden whose input was deleted would otherwise sit unchecked forever.
    let orphans: Vec<&PathBuf> = goldens
        .iter()
        .filter(|golden_path| {
            let stem = golden_path.file_stem().unwrap().to_string_lossy();
            let base = stem.strip_suffix("-formatted").unwrap();
            !golden_path.with_file_name(format!("{base}.spade")).exists()
        })
        .collect();
    if bless {
        for orphan in orphans {
            fs::remove_file(orphan).unwrap_or_else(|error| {
                panic!("failed to delete orphan {orphan:?}: {error}")
            });
        }
    } else {
        assert!(
            orphans.is_empty(),
            "goldens without an input twin: {orphans:?}; restore their \
             inputs or delete them (a bless run does)"
        );
    }
}

#[test]
fn idempotency() {
    let config = config();
    let inputs = corpus_inputs();

    for entry in KNOWN_NON_IDEMPOTENT {
        assert!(
            inputs.iter().any(|input| {
                input.file_name().unwrap().to_string_lossy() == *entry
            }),
            "{entry} is listed in KNOWN_NON_IDEMPOTENT but is not a corpus \
             input"
        );
    }

    for input in inputs {
        let file_name = input.file_name().unwrap().to_string_lossy();
        let source = format!("{} (reformatted)", input.display());
        let once = format_file(&input, &config);
        let twice = format_str(&once, &source, &config);

        if KNOWN_NON_IDEMPOTENT.contains(&file_name.as_ref()) {
            assert_ne!(
                once, twice,
                "{file_name} is now idempotent; remove it from \
                 KNOWN_NON_IDEMPOTENT"
            );
        } else {
            assert_text_eq(
                &twice,
                &once,
                &format!("reformatting {file_name} changed its output"),
            );
        }
    }
}

#[test]
fn must_not_panic() {
    let config = config();
    let mut files = vec![];
    all_spade_files(&asts_dir(), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no .spade files found under asts/");

    let mut unvisited: Vec<&str> = KNOWN_PANICS.to_vec();
    let mut failures = vec![];
    for path in &files {
        let relative = path
            .strip_prefix(asts_dir())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let code = match fs::read_to_string(path) {
            Ok(code) => code,
            // Deleted between enumeration and read, e.g. by a concurrent
            // bless run cleaning up an orphan golden.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => panic!("failed to read {path:?}: {error}"),
        };

        // Parse errors are fine here: files only must not panic. Expected
        // panics from KNOWN_PANICS fixtures print a panic message, but
        // libtest discards captured output when the test passes.
        let panicked = panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = format_source(&relative, &code, &config, false);
        }))
        .is_err();

        unvisited.retain(|entry| *entry != relative);
        match (panicked, KNOWN_PANICS.contains(&relative.as_str())) {
            (true, false) => {
                failures.push(format!("{relative}: formatting panicked"))
            }
            (false, true) => failures.push(format!(
                "{relative}: no longer panics; remove it from KNOWN_PANICS"
            )),
            _ => {}
        }
    }
    // A plain clone (no `git submodule update --init`) leaves swim-templates
    // empty; its entries are then absent by design, not stale.
    let swim_populated = files
        .iter()
        .any(|file| file.starts_with(asts_dir().join("swim-templates")));
    for entry in unvisited {
        if !swim_populated && entry.starts_with("swim-templates/") {
            continue;
        }
        failures.push(format!("{entry}: listed in KNOWN_PANICS but not swept"));
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
