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
    fmt::{self, Write},
};

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub struct DocumentIdx(usize);

#[derive(PartialEq, Eq, Hash, Clone)]
pub enum Document {
    Newline,
    Text(String),
    Nest(DocumentIdx, isize),
    Flatten(DocumentIdx),
    List(Vec<DocumentIdx>),
    TryCatch(DocumentIdx, DocumentIdx),
    Raw(String),
    /// Reaches the output byte for byte: no indentation, no whitespace
    /// handling (string literals).
    Verbatim(String),
}

#[derive(Default)]
pub struct InternedDocumentStore {
    documents: Vec<Document>,
    inverse: HashMap<Document, DocumentIdx>,
}

impl InternedDocumentStore {
    pub fn add(&mut self, document: Document) -> DocumentIdx {
        if let Some(existing_idx) = self.inverse.get(&document) {
            *existing_idx
        } else {
            self.documents.push(document.clone());
            let new_idx = DocumentIdx(self.documents.len() - 1);
            self.inverse.insert(document, new_idx);
            new_idx
        }
    }

    pub fn get(&self, idx: DocumentIdx) -> &Document {
        &self.documents[idx.0]
    }

    pub fn get_mut(&mut self, idx: DocumentIdx) -> &mut Document {
        &mut self.documents[idx.0]
    }
}

/// Indenting writer that holds indentation and whitespace back until
/// non-whitespace content follows on the same line, so no line ends in
/// whitespace and a whitespace-only line is empty. Every line of a
/// [`fmt::Write`] write is indented to the current depth; [`Self::verbatim`]
/// bypasses all of it.
pub struct Writer<W> {
    out: W,
    indent: usize,
    at_line_start: bool,
    pending: String,
}

impl<W: fmt::Write> Writer<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            indent: 0,
            at_line_start: true,
            pending: String::new(),
        }
    }

    fn indent(&mut self, by: isize) {
        self.indent = (self.indent as isize + by) as usize;
    }

    /// Opens the line's content: the indent, then the held-back whitespace.
    fn flush(&mut self) -> fmt::Result {
        if self.at_line_start {
            self.at_line_start = false;
            for _ in 0..self.indent {
                self.out.write_char(' ')?;
            }
        }
        self.out.write_str(&self.pending)?;
        self.pending.clear();
        Ok(())
    }

    fn line_part(&mut self, part: &str) -> fmt::Result {
        let content = part.trim_end_matches([' ', '\t']);
        if !content.is_empty() {
            self.flush()?;
            self.out.write_str(content)?;
        }
        self.pending.push_str(&part[content.len()..]);
        Ok(())
    }

    fn newline(&mut self) -> fmt::Result {
        self.pending.clear();
        self.at_line_start = true;
        self.out.write_char('\n')
    }

    pub fn verbatim(&mut self, text: &str) -> fmt::Result {
        if text.is_empty() {
            return Ok(());
        }
        self.flush()?;
        self.out.write_str(text)?;
        self.at_line_start = text.ends_with('\n');
        Ok(())
    }
}

impl<W: fmt::Write> fmt::Write for Writer<W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for (i, part) in s.split('\n').enumerate() {
            if i > 0 {
                self.newline()?;
            }
            self.line_part(part)?;
        }
        Ok(())
    }
}

pub fn print_resolved<W: fmt::Write>(
    store: &InternedDocumentStore,
    f: &mut Writer<W>,
    idx: DocumentIdx,
    flattened: bool,
    last_was_newline: &mut bool,
) -> fmt::Result {
    let last_was_newline_old = *last_was_newline;
    *last_was_newline = false;
    match store.get(idx) {
        Document::Newline => {
            if flattened {
                if !last_was_newline_old {
                    write!(f, " ")?;
                }
            } else {
                writeln!(f)?;
            }
            *last_was_newline = true;

            Ok(())
        }
        Document::Text(text) | Document::Raw(text) => f.write_str(text),
        Document::Verbatim(text) => f.verbatim(text),
        Document::Nest(body_idx, by) => {
            f.indent(*by);
            let result = print_resolved(
                store,
                f,
                *body_idx,
                flattened,
                last_was_newline,
            );
            f.indent(-by);
            result
        }
        Document::Flatten(body_idx) => {
            print_resolved(store, f, *body_idx, true, last_was_newline)
        }
        Document::List(children) => {
            children.iter().copied().try_for_each(|child| {
                print_resolved(store, f, child, flattened, last_was_newline)
            })
        }
        Document::TryCatch(_, _) => {
            panic!("TryCatch found in resolved document")
        }
    }
}

const DEBUG_INDENT: isize = 4;

pub fn debug_print<W: fmt::Write>(
    store: &InternedDocumentStore,
    f: &mut Writer<W>,
    idx: DocumentIdx,
) -> fmt::Result {
    match store.get(idx) {
        Document::Newline => write!(f, "Newline"),
        Document::Text(text) => write!(f, "Text(\"{text}\")"),
        Document::Nest(body_idx, by) => {
            writeln!(f, "Nest(")?;
            f.indent(DEBUG_INDENT);
            debug_print(store, f, *body_idx)?;
            writeln!(f, ",\n{by}")?;
            f.indent(-DEBUG_INDENT);
            write!(f, ")")
        }
        Document::Flatten(body_idx) => {
            writeln!(f, "Flatten(")?;
            f.indent(DEBUG_INDENT);
            debug_print(store, f, *body_idx)?;
            writeln!(f)?;
            f.indent(-DEBUG_INDENT);
            write!(f, ")")
        }
        Document::List(children) => {
            if children.is_empty() {
                return Ok(());
            }
            writeln!(f, "List(")?;
            f.indent(DEBUG_INDENT);
            for child in children {
                debug_print(store, f, *child)?;
                writeln!(f, ",")?;
            }
            f.indent(-DEBUG_INDENT);
            write!(f, ")")
        }
        Document::TryCatch(try_body, catch_body) => {
            writeln!(f, "TryCatch(")?;
            f.indent(DEBUG_INDENT);
            debug_print(store, f, *try_body)?;
            writeln!(f, ",")?;
            debug_print(store, f, *catch_body)?;
            writeln!(f, ",")?;
            f.indent(-DEBUG_INDENT);
            write!(f, ")")
        }
        Document::Raw(raw) => write!(f, "Raw(\"{raw}\")"),
        Document::Verbatim(text) => write!(f, "Verbatim(\"{text}\")"),
    }
}
