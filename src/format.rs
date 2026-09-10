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

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};

use itertools::Itertools;
use snafu::{ResultExt, Snafu};
use spade::error_handling::{ErrorHandler, Reportable};
use spade_ast::ModuleBody;
use spade_codespan_reporting::{files::SimpleFiles, term::termcolor::Buffer};
use spade_common::location_info::Loc;
use spade_diagnostics::{
    CodeBundle, DiagHandler, Diagnostic, emitter::CodespanEmitter,
};
use spade_parser::Comment;

use crate::{
    comment_insertion::CommentMap,
    config::Config,
    document::{self, DocumentIdx, InternedDocumentStore},
    document_builder::DocumentBuilder,
    resolve_try_catch::{PrintingContext, resolve_try_catch},
};

/// The result of successfully formatting a source file.
pub struct Formatted {
    /// Formatted source with no trailing whitespace, ending with a newline;
    /// empty for an input with no content.
    pub text: String,
    /// Rendered non-fatal parser diagnostics, empty if there were none.
    pub diagnostics: String,
    /// Comments no claim site attached anywhere; they are appended at the
    /// end of the output instead (never lost). Nonzero means a comment
    /// placement bug.
    pub unclaimed_comments: usize,
}

#[derive(Debug, Snafu)]
pub enum FormatError {
    /// Parsing failed; `diagnostics` holds the rendered errors.
    #[snafu(display("Failed to parse input"))]
    Parse { diagnostics: String },
    /// The input contains constructs the document builder does not support
    /// yet; `diagnostics` holds the rendered errors, one per construct.
    #[snafu(display("Input contains constructs spadefmt cannot format yet"))]
    Unsupported { diagnostics: String },
    #[snafu(display("Failed to print document"))]
    Print { source: fmt::Error },
}

/// A successfully parsed source file, ready for formatting.
///
/// Parsing is split from formatting so callers can flush `diagnostics`
/// before [`Parsed::format`], which reports its own diagnostics on
/// constructs the document builder does not support yet.
pub struct Parsed {
    code: String,
    root: Loc<ModuleBody>,
    comments: Vec<Comment>,
    code_bundle: Arc<RwLock<CodeBundle>>,
    file_id: usize,
    color: bool,
    /// Rendered non-fatal parser diagnostics, empty if there were none.
    pub diagnostics: String,
}

/// Runs `operation` with an [`ErrorHandler`] rendering into a fresh buffer
/// and returns its result alongside the rendered diagnostics. `color`
/// enables ANSI colors.
fn with_error_handler<R>(
    code_bundle: &Arc<RwLock<CodeBundle>>,
    color: bool,
    operation: impl FnOnce(&mut ErrorHandler) -> R,
) -> (R, String) {
    let mut buffer = if color {
        Buffer::ansi()
    } else {
        Buffer::no_color()
    };
    let diagnostic_handler = DiagHandler::new(Box::new(CodespanEmitter));
    let mut error_handler =
        ErrorHandler::new(&mut buffer, diagnostic_handler, code_bundle.clone());
    let result = operation(&mut error_handler);
    drop(error_handler);
    let rendered = String::from_utf8_lossy(buffer.as_slice()).into_owned();
    (result, rendered)
}

/// Parses `code` (from `file_name`, used in diagnostics). `color` enables
/// ANSI colors in diagnostics.
pub fn parse_source(
    file_name: &str,
    code: &str,
    color: bool,
) -> Result<Parsed, FormatError> {
    let mut files = SimpleFiles::new();
    let file_id = files.add(file_name.to_string(), code.to_string());

    let code_bundle = Arc::new(RwLock::new(CodeBundle {
        files,
        file_ids: HashMap::from_iter([(file_name.to_string(), file_id)]),
    }));

    let mut parser = spade_parser::Parser::new(code, file_id, None);

    let ((root_opt, failed), diagnostics) =
        with_error_handler(&code_bundle, color, |error_handler| {
            let root_opt =
                parser.top_level_module_body().or_report(error_handler);
            error_handler.drain_diag_list(&mut parser.diags);
            // The parser can recover from errors and return a module body
            // with the offending items dropped; formatting that would
            // silently delete code.
            (root_opt, error_handler.failed())
        });

    let Some(root) = root_opt.filter(|_| !failed) else {
        return ParseSnafu { diagnostics }.fail();
    };

    Ok(Parsed {
        code: code.to_string(),
        root,
        comments: parser.comments().to_vec(),
        code_bundle,
        file_id,
        color,
        diagnostics,
    })
}

impl Parsed {
    fn build(
        &self,
        config: &Config,
    ) -> (InternedDocumentStore, DocumentIdx, Vec<Diagnostic>, usize) {
        let code_bundle_guard = self.code_bundle.read().unwrap();
        let file = code_bundle_guard.files.get(self.file_id).unwrap();
        let mut comments = CommentMap::new(&self.comments, &self.code);
        let (store, root_idx, diagnostics) = DocumentBuilder::new(
            config.indent.inner as isize,
        )
        .build_root(&self.root, file, &mut comments);
        (store, root_idx, diagnostics, comments.misplaced())
    }

    /// Renders `diagnostics` the same way parse-time ones are rendered.
    fn render_diagnostics(&self, diagnostics: &[Diagnostic]) -> String {
        with_error_handler(&self.code_bundle, self.color, |error_handler| {
            for diagnostic in diagnostics {
                error_handler.report(diagnostic);
            }
        })
        .1
    }

    /// Renders the formatted source according to `config`.
    pub fn format(&self, config: &Config) -> Result<Formatted, FormatError> {
        let (mut store, root_idx, unsupported, unclaimed_comments) =
            self.build(config);
        if !unsupported.is_empty() {
            return UnsupportedSnafu {
                diagnostics: self.render_diagnostics(&unsupported),
            }
            .fail();
        }

        let new_root_idx = resolve_try_catch(
            &mut store,
            root_idx,
            &mut PrintingContext::new(config.max_width.inner),
        );

        let mut buffer = String::new();
        let mut f =
            inform::fmt::IndentWriter::new(&mut buffer, config.indent.inner);
        document::print_resolved(
            &store,
            &mut f,
            new_root_idx,
            false,
            &mut false,
        )
        .context(PrintSnafu)?;

        Ok(Formatted {
            text: into_clean_text(buffer),
            diagnostics: self.diagnostics.clone(),
            unclaimed_comments,
        })
    }

    /// Renders the unresolved document tree for debugging.
    pub fn debug_document(
        &self,
        config: &Config,
    ) -> Result<String, FormatError> {
        let (store, root_idx, unsupported, _) = self.build(config);
        if !unsupported.is_empty() {
            return UnsupportedSnafu {
                diagnostics: self.render_diagnostics(&unsupported),
            }
            .fail();
        }

        let mut buffer = String::new();
        let mut f =
            inform::fmt::IndentWriter::new(&mut buffer, config.indent.inner);
        document::debug_print(&store, &mut f, root_idx).context(PrintSnafu)?;

        Ok(into_clean_text(buffer))
    }
}

/// Appends a final newline and strips trailing spaces, tabs, and carriage
/// returns from every line. Stopping at that set keeps other trailing
/// characters (e.g. no-break spaces inside comments) intact. A render with
/// no content at all (an empty or whitespace-only input) is the empty
/// string, so an empty file is a fixed point.
fn into_clean_text(rendered: String) -> String {
    if rendered.trim().is_empty() {
        return String::new();
    }
    rendered
        .split('\n')
        .map(|line| line.trim_end_matches([' ', '\t', '\r']))
        .join("\n")
        + "\n"
}

/// Parses and formats `code` in one step; see [`parse_source`] and
/// [`Parsed::format`].
pub fn format_source(
    file_name: &str,
    code: &str,
    config: &Config,
    color: bool,
) -> Result<Formatted, FormatError> {
    parse_source(file_name, code, color)?.format(config)
}
