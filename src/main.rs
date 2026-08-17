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
    env, fs,
    io::{self, IsTerminal},
};

use snafu::{ResultExt, Whatever, whatever};
use spadefmt::{
    cli::Opts,
    config::Config,
    format::{FormatError, parse_source},
};

#[snafu::report]
fn main() -> Result<(), Whatever> {
    let mut opts = Opts::from_env();
    opts.no_color |= env::var("NO_COLOR")
        .map(|var| var.trim() == "1")
        .unwrap_or(false);

    if opts.version {
        println!(
            "{} {}",
            env::args().next().expect("no program name"),
            env!("CARGO_PKG_VERSION")
        );
        println!();
        print!(include_str!("../resources/version.txt"));

        return Ok(());
    }

    let code = fs::read_to_string(&opts.file)
        .whatever_context(format!("Failed to read file at {}", opts.file))?;

    let config_contents = fs::read_to_string("spadefmt.toml")
        .whatever_context("test file spadefmt.toml should be there")?;
    let config = toml::from_str::<Config>(&config_contents)
        .whatever_context("Failed to decode config")?;

    let color = !opts.no_color && io::stderr().is_terminal();

    let parsed = match parse_source(opts.file.as_str(), &code, color) {
        Ok(parsed) => parsed,
        Err(FormatError::Parse { diagnostics }) => {
            print!("{diagnostics}");
            whatever!("Exiting due to errors")
        }
        Err(error) => {
            return Err(error).whatever_context("Failed to parse input");
        }
    };
    // Flushed before formatting, which can still panic on unsupported
    // constructs.
    print!("{}", parsed.diagnostics);

    let output = if opts.debug {
        parsed.debug_document(&config)
    } else {
        parsed.format(&config)
    };
    print!("{}", output.whatever_context("Failed to print document")?);

    Ok(())
}
