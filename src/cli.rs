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

use std::{env, ffi::OsStr, path::Path};

use argh::{EarlyExit, FromArgs};
use camino::Utf8PathBuf;

/// The input naming stdin.
pub const STDIN: &str = "-";

/// Format Spade code
#[derive(FromArgs)]
pub struct Opts {
    /// rewrite the inputs instead of printing to stdout; only files whose
    /// text changes are written
    #[argh(switch, short = 'i')]
    pub in_place: bool,

    /// write nothing; list every file that would change and exit 1 if there
    /// is any
    #[argh(switch)]
    pub check: bool,

    /// config file to use instead of the nearest spadefmt.toml
    #[argh(option, arg_name = "path")]
    pub config: Option<Utf8PathBuf>,

    /// disable colored output
    #[argh(switch)]
    pub no_color: bool,

    /// print debug representation
    #[argh(switch)]
    pub debug: bool,

    /// show version information
    #[argh(switch, short = 'v')]
    pub version: bool,

    /// files or directories to format; a directory is searched for *.spade
    /// files, and `-` reads stdin and writes stdout
    #[argh(positional)]
    pub files: Vec<Utf8PathBuf>,
}

impl Opts {
    /// Parses the process arguments. A usage error or `--help` comes back as
    /// an [`EarlyExit`] so the caller chooses the exit code.
    ///
    /// argh reads a bare `-` as an option, so before `--` each one is
    /// parsed through a placeholder and mapped back afterwards. That keeps
    /// the inputs in the order given, and a `-` following an option that
    /// takes a value (`--config -`) is that option's value, a file called
    /// `-`, rather than stdin.
    pub fn from_env() -> Result<Self, EarlyExit> {
        // No real argument holds a NUL byte.
        const PLACEHOLDER: &str = "\0";
        let args = env::args_os()
            .skip(1)
            .map(|arg| {
                arg.into_string().map_err(|arg| {
                    EarlyExit::from(format!(
                        "Invalid UTF-8 in argument: {}",
                        arg.to_string_lossy()
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let options_end = args
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(args.len());
        let args: Vec<&str> = args
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                if index < options_end && arg == STDIN {
                    PLACEHOLDER
                } else {
                    arg
                }
            })
            .collect();
        let mut opts = Self::from_args(&[&program_name()], &args)?;
        let restore = |path: &mut Utf8PathBuf| {
            if path == PLACEHOLDER {
                *path = STDIN.into();
            }
        };
        opts.files.iter_mut().for_each(restore);
        opts.config.iter_mut().for_each(restore);
        Ok(opts)
    }
}

/// The bare name the process was invoked as.
pub fn program_name() -> String {
    let program = env::args_os().next().unwrap_or_default();
    Path::new(&program)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(env!("CARGO_PKG_NAME"))
        .to_owned()
}

/// Whether stderr diagnostics get ANSI colors: never with `--no-color`, with
/// `NO_COLOR` set to a non-empty value, or off a terminal.
pub fn color_enabled(
    no_color_flag: bool,
    no_color_env: Option<&OsStr>,
    stderr_is_terminal: bool,
) -> bool {
    !no_color_flag
        && no_color_env.is_none_or(OsStr::is_empty)
        && stderr_is_terminal
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::color_enabled;

    #[test]
    fn no_color_disables_on_any_non_empty_value() {
        assert!(color_enabled(false, None, true));
        assert!(color_enabled(false, Some(OsStr::new("")), true));
        assert!(!color_enabled(false, Some(OsStr::new("1")), true));
        assert!(!color_enabled(false, Some(OsStr::new("anything")), true));
        assert!(!color_enabled(true, None, true));
        assert!(!color_enabled(false, None, false));
    }
}
