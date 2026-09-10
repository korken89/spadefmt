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
    borrow::Cow,
    collections::HashMap,
    fmt,
    sync::{Arc, RwLock},
};

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
    document::{self, DocumentIdx, InternedDocumentStore, Writer},
    document_builder::DocumentBuilder,
    resolve_try_catch::{PrintingContext, resolve_try_catch},
};

/// The result of successfully formatting a source file.
pub struct Formatted {
    /// Formatted source with LF line endings, ending with a newline, and no
    /// trailing whitespace outside string literals; empty for an input with
    /// no content.
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

/// `code` with `\r\n` line endings normalized to `\n`, the form every
/// formatting entry point parses and emits.
pub fn normalize_line_endings(code: &str) -> Cow<'_, str> {
    if code.contains("\r\n") {
        Cow::Owned(code.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(code)
    }
}

/// Parses `code` (from `file_name`, used in diagnostics) with its line
/// endings normalized. `color` enables ANSI colors in diagnostics.
pub fn parse_source(
    file_name: &str,
    code: &str,
    color: bool,
) -> Result<Parsed, FormatError> {
    let code = normalize_line_endings(code);
    let mut files = SimpleFiles::new();
    let file_id = files.add(file_name.to_string(), code.to_string());

    let code_bundle = Arc::new(RwLock::new(CodeBundle {
        files,
        file_ids: HashMap::from_iter([(file_name.to_string(), file_id)]),
    }));

    let mut parser = spade_parser::Parser::new(&code, file_id, None);

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

    let comments = parser.comments().to_vec();
    Ok(Parsed {
        code: code.into_owned(),
        root,
        comments,
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
        document::print_resolved(
            &store,
            &mut Writer::new(&mut buffer),
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
        document::debug_print(&store, &mut Writer::new(&mut buffer), root_idx)
            .context(PrintSnafu)?;

        Ok(into_clean_text(buffer))
    }
}

/// Appends the final newline. A render with no content at all (an empty or
/// whitespace-only input) is the empty string, so an empty file is a fixed
/// point.
fn into_clean_text(rendered: String) -> String {
    if rendered.trim().is_empty() {
        return String::new();
    }
    rendered + "\n"
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
