// Copyright (C) 2024 Ethan Uppal.
//
// This file is part of spadefmt.
//
// spadefmt is free software: you can redistribute it and/or modify it under the
// terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version. spadefmt is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more
// details. You should have received a copy of the GNU General Public License
// along with spadefmt. If not, see <https://www.gnu.org/licenses/>.

#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    env,
    error::Error,
    fs,
    io::{self, IsTerminal, Read, StdoutLock, Write},
    ops::ControlFlow::{self, Break, Continue},
    path::PathBuf,
    process::ExitCode,
};

use argh::EarlyExit;
use camino::{Utf8Path, Utf8PathBuf};
use snafu::{IntoError, ResultExt, Snafu};
use spade_codespan_reporting::term::termcolor::{
    Color, ColorChoice, ColorSpec, StandardStream, WriteColor,
};
use spadefmt::{
    cli::{self, Opts},
    config::Config,
    format::{FormatError, parse_source},
    walk,
};

const STDIN: &str = "<stdin>";

/// Exit codes, ordered by severity: a run over several inputs exits with its
/// worst outcome.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Exit {
    Clean = 0,
    /// `--check` found a file that would change.
    Unformatted = 1,
    /// Usage, IO, or config error.
    Failure = 2,
    ParseError = 3,
    Unsupported = 4,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        Self::from(exit as u8)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Formatted text to stdout, one input only.
    Stdout,
    /// Unresolved document tree to stdout, one input only.
    Debug,
    InPlace,
    Check,
}

#[derive(Debug, Snafu)]
enum CliError {
    #[snafu(display("{message}"))]
    Usage { message: String },
    #[snafu(display("failed to read {path}"))]
    Read {
        path: Utf8PathBuf,
        source: io::Error,
    },
    #[snafu(display("failed to write {path}"))]
    Write {
        path: Utf8PathBuf,
        source: io::Error,
    },
    #[snafu(display("cannot determine the directory of {path}"))]
    Locate {
        path: Utf8PathBuf,
        source: io::Error,
    },
    #[snafu(display(
        "{path} is a directory; use --check or --in-place to format directories"
    ))]
    Directory { path: Utf8PathBuf },
    #[snafu(display("{} is not a UTF-8 path", path.display()))]
    NonUtf8Path { path: PathBuf },
    #[snafu(display("failed to write stdout"))]
    Stdout { source: io::Error },
}

/// Stderr, printing spadefmt's own errors in the codespan `error:` style.
struct Stderr(StandardStream);

impl Stderr {
    fn new(color: bool) -> Self {
        let choice = if color {
            ColorChoice::Always
        } else {
            ColorChoice::Never
        };
        Self(StandardStream::stderr(choice))
    }

    /// `error: <message>`, then one `caused by:` line per source.
    fn error(&mut self, error: &dyn Error) {
        let _ = self.try_error(error);
    }

    fn try_error(&mut self, error: &dyn Error) -> io::Result<()> {
        let mut header = ColorSpec::new();
        header.set_bold(true).set_intense(true);
        self.0.set_color(header.clone().set_fg(Some(Color::Red)))?;
        write!(self.0, "error")?;
        self.0.set_color(&header)?;
        write!(self.0, ": {error}")?;
        self.0.reset()?;
        writeln!(self.0)?;
        let mut source = error.source();
        while let Some(cause) = source {
            writeln!(self.0, "caused by: {}", cause.to_string().trim_end())?;
            source = cause.source();
        }
        Ok(())
    }

    /// A usage error with the `--help` hint.
    fn usage(&mut self, message: &str) -> ExitCode {
        self.error(
            &UsageSnafu {
                message: message.trim_end(),
            }
            .build(),
        );
        let _ = writeln!(
            self.0,
            "Run {} --help for more information.",
            env!("CARGO_PKG_NAME")
        );
        Exit::Failure.into()
    }

    /// Codespan-rendered diagnostics, already styled.
    fn diagnostics(&mut self, rendered: &str) {
        let _ = write!(self.0, "{rendered}");
    }

    /// The exit of a write to stdout: a reader that went away ends the run
    /// quietly, any other failure is reported.
    fn stdout(&mut self, written: io::Result<()>) -> Exit {
        match written {
            Ok(()) => Exit::Clean,
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
                Exit::Clean
            }
            Err(source) => {
                self.error(&StdoutSnafu.into_error(source));
                Exit::Failure
            }
        }
    }
}

/// Config lookup caching each config file by path; a broken file is reported
/// once and then remembered as unusable.
struct Configs {
    explicit: Option<Utf8PathBuf>,
    loaded: HashMap<Utf8PathBuf, Option<Config>>,
}

impl Configs {
    /// The config governing files in the absolute `directory`; `None` after
    /// reporting a broken config file.
    fn for_directory(
        &mut self,
        directory: &Utf8Path,
        stderr: &mut Stderr,
    ) -> Option<Config> {
        let path = match &self.explicit {
            Some(path) => path.clone(),
            None => match Config::discover(directory) {
                Some(path) => path,
                None => return Some(Config::default()),
            },
        };
        if let Some(cached) = self.loaded.get(&path) {
            return *cached;
        }
        let config = Config::load(&path)
            .inspect_err(|error| stderr.error(error))
            .ok();
        self.loaded.insert(path, config);
        config
    }
}

#[derive(Clone, Copy)]
enum Input<'a> {
    Stdin,
    File(&'a Utf8Path),
}

impl Input<'_> {
    /// The name diagnostics and `--check` report.
    fn name(&self) -> &str {
        match self {
            Self::Stdin => STDIN,
            Self::File(path) => path.as_str(),
        }
    }

    fn read(self) -> Result<String, CliError> {
        match self {
            Self::Stdin => {
                let mut code = String::new();
                io::stdin()
                    .read_to_string(&mut code)
                    .context(ReadSnafu { path: STDIN })?;
                Ok(code)
            }
            Self::File(path) => {
                fs::read_to_string(path).context(ReadSnafu { path })
            }
        }
    }

    /// The absolute directory config discovery starts from.
    fn directory(self) -> Result<Utf8PathBuf, CliError> {
        match self {
            Self::Stdin => env::current_dir()
                .and_then(|directory| {
                    Utf8PathBuf::try_from(directory)
                        .map_err(|error| error.into_io_error())
                })
                .context(LocateSnafu { path: STDIN }),
            Self::File(path) => {
                let absolute = camino::absolute_utf8(path)
                    .context(LocateSnafu { path })?;
                let parent = absolute.parent().map(Utf8Path::to_path_buf);
                Ok(parent.unwrap_or(absolute))
            }
        }
    }
}

/// The worst of a sequence of outcomes; `Break` ends the run early with
/// the outcomes so far folded in.
fn worst_of(
    outcomes: impl IntoIterator<Item = ControlFlow<Exit, Exit>>,
) -> ControlFlow<Exit, Exit> {
    let mut worst = Exit::Clean;
    for outcome in outcomes {
        match outcome {
            Continue(exit) => worst = worst.max(exit),
            Break(exit) => return Break(worst.max(exit)),
        }
    }
    Continue(worst)
}

struct Session {
    mode: Mode,
    color: bool,
    stdout: StdoutLock<'static>,
    stderr: Stderr,
    configs: Configs,
}

impl Session {
    fn run(&mut self, inputs: &[Utf8PathBuf]) -> Exit {
        match worst_of(inputs.iter().map(|input| self.process_input(input))) {
            Continue(exit) | Break(exit) => exit,
        }
    }

    fn process_input(&mut self, input: &Utf8Path) -> ControlFlow<Exit, Exit> {
        if input.as_str() == cli::STDIN {
            let outcome = self.process(Input::Stdin);
            self.settle(outcome)
        } else if input.is_dir() {
            self.process_directory(input)
        } else {
            let outcome = self.process(Input::File(input));
            self.settle(outcome)
        }
    }

    /// Reports an error and maps it to its exit code. A failed stdout write
    /// ends the run: nothing more can be printed.
    fn settle(
        &mut self,
        outcome: Result<Exit, CliError>,
    ) -> ControlFlow<Exit, Exit> {
        match outcome {
            Ok(exit) => Continue(exit),
            Err(CliError::Stdout { source }) => {
                Break(self.stderr.stdout(Err(source)))
            }
            Err(error) => {
                self.stderr.error(&error);
                Continue(Exit::Failure)
            }
        }
    }

    fn process_directory(
        &mut self,
        directory: &Utf8Path,
    ) -> ControlFlow<Exit, Exit> {
        if matches!(self.mode, Mode::Stdout | Mode::Debug) {
            return self.settle(DirectorySnafu { path: directory }.fail());
        }
        let files = match walk::spade_files(directory.as_std_path())
            .context(ReadSnafu { path: directory })
        {
            Ok(files) => files,
            Err(error) => return self.settle(Err(error)),
        };
        worst_of(files.into_iter().map(|file| {
            let outcome = Utf8PathBuf::try_from(file)
                .map_err(|error| {
                    NonUtf8PathSnafu {
                        path: error.into_path_buf(),
                    }
                    .build()
                })
                .and_then(|file| self.process(Input::File(&file)));
            self.settle(outcome)
        }))
    }

    fn process(&mut self, input: Input) -> Result<Exit, CliError> {
        let code = input.read()?;
        let directory = input.directory()?;
        let Some(config) =
            self.configs.for_directory(&directory, &mut self.stderr)
        else {
            return Ok(Exit::Failure);
        };
        let text = match self.format(input.name(), &code, &config) {
            Ok(text) => text,
            Err(exit) => return Ok(exit),
        };
        Ok(match self.mode {
            Mode::Stdout | Mode::Debug => {
                self.stdout
                    .write_all(text.as_bytes())
                    .context(StdoutSnafu)?;
                Exit::Clean
            }
            Mode::Check if text != code => {
                writeln!(self.stdout, "would reformat: {}", input.name())
                    .context(StdoutSnafu)?;
                Exit::Unformatted
            }
            Mode::Check => Exit::Clean,
            Mode::InPlace => match input {
                Input::File(path) if text != code => {
                    fs::write(path, text).context(WriteSnafu { path })?;
                    Exit::Clean
                }
                Input::File(_) => Exit::Clean,
                Input::Stdin => {
                    self.stdout
                        .write_all(text.as_bytes())
                        .context(StdoutSnafu)?;
                    Exit::Clean
                }
            },
        })
    }

    /// Parses and formats `code`, printing every diagnostic; `Err` carries
    /// the exit code of a failed format.
    fn format(
        &mut self,
        name: &str,
        code: &str,
        config: &Config,
    ) -> Result<String, Exit> {
        let parsed = match parse_source(name, code, self.color) {
            Ok(parsed) => parsed,
            Err(FormatError::Parse { diagnostics }) => {
                self.stderr.diagnostics(&diagnostics);
                return Err(Exit::ParseError);
            }
            Err(error) => {
                self.stderr.error(&error);
                return Err(Exit::Failure);
            }
        };
        // Flushed before formatting, which reports its own diagnostics on
        // unsupported constructs.
        self.stderr.diagnostics(&parsed.diagnostics);
        let output = if self.mode == Mode::Debug {
            parsed.debug_document(config)
        } else {
            parsed.format(config).map(|formatted| formatted.text)
        };
        output.map_err(|error| match error {
            FormatError::Unsupported { diagnostics } => {
                self.stderr.diagnostics(&diagnostics);
                Exit::Unsupported
            }
            error => {
                self.stderr.error(&error);
                Exit::Failure
            }
        })
    }
}

fn main() -> ExitCode {
    let stderr_is_terminal = io::stderr().is_terminal();
    let no_color_env = env::var_os("NO_COLOR");
    let color_for = |no_color_flag| {
        cli::color_enabled(
            no_color_flag,
            no_color_env.as_deref(),
            stderr_is_terminal,
        )
    };

    let mut stdout = io::stdout().lock();
    let opts = match Opts::from_env() {
        Ok(opts) => opts,
        Err(EarlyExit {
            output,
            status: Ok(()),
        }) => {
            let written = writeln!(stdout, "{}", output.trim_end());
            return Stderr::new(color_for(false)).stdout(written).into();
        }
        Err(EarlyExit { output, .. }) => {
            return Stderr::new(color_for(false)).usage(&output);
        }
    };
    let color = color_for(opts.no_color);
    let mut stderr = Stderr::new(color);

    if opts.version {
        let written = write!(
            stdout,
            "{} {}\n\n{}",
            cli::program_name(),
            env!("CARGO_PKG_VERSION"),
            include_str!("../resources/version.txt")
        );
        return stderr.stdout(written).into();
    }

    let mode = match (opts.check, opts.in_place, opts.debug) {
        (true, true, _) => {
            return stderr
                .usage("--check and --in-place are mutually exclusive");
        }
        (true, _, true) | (_, true, true) => {
            return stderr.usage(
                "--debug prints to stdout and cannot be combined with --check \
                 or --in-place",
            );
        }
        (true, false, false) => Mode::Check,
        (false, true, false) => Mode::InPlace,
        (false, false, true) => Mode::Debug,
        (false, false, false) => Mode::Stdout,
    };
    if opts.files.is_empty() {
        return stderr.usage("no input files");
    }
    if matches!(mode, Mode::Stdout | Mode::Debug) && opts.files.len() > 1 {
        return stderr.usage(
            "formatting to stdout takes exactly one input; use --check or \
             --in-place for several",
        );
    }

    let mut session = Session {
        mode,
        color,
        stdout,
        stderr,
        configs: Configs {
            explicit: opts.config,
            loaded: HashMap::new(),
        },
    };
    session.run(&opts.files).into()
}
