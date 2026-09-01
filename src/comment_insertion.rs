// Copyright (C) 2025 Ethan Uppal.
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

use std::ops::Range;

use spade_parser::Comment;

/// Separators and closers; a single-line block comment followed by one of
/// these attaches backward (trailing), otherwise forward (inline).
const CLOSERS: &[u8] = b",;)}]>";

/// How a comment relates to the code around it, decided once from source
/// coordinates.
#[derive(Clone, Copy, PartialEq)]
enum CommentClass {
    /// On its own line(s): prints on its own line above the next construct.
    OwnLine,
    /// A single-line `/* */` directly before code: prints inline before it.
    InlineLeading,
    /// Code precedes it on its line: prints after that code.
    Trailing,
}

struct MappedComment<'source> {
    text: &'source str,
    span: Range<usize>,
    start_line: usize,
    end_line: usize,
    is_line: bool,
    class: CommentClass,
    claimed: bool,
}

/// A claimed comment, ready to print.
pub struct CommentToPrint<'source> {
    pub text: &'source str,
    pub start_line: usize,
    pub end_line: usize,
    /// Print inline (followed by a space) rather than on its own line.
    pub inline: bool,
    /// A `//` comment: nothing can print after it on its line.
    pub is_line: bool,
}

/// Owns every parser comment and hands each to exactly one claim site,
/// keyed by source byte positions. Whatever is never claimed is emitted at
/// the end of the root (comments are never lost) and counted as misplaced.
pub struct CommentMap<'source> {
    source: &'source str,
    line_starts: Vec<usize>,
    comments: Vec<MappedComment<'source>>,
    misplaced: usize,
}

impl<'source> CommentMap<'source> {
    pub fn new(comments: &[Comment], source: &'source str) -> Self {
        let mut line_starts = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }

        let mut raw: Vec<(Range<usize>, bool)> = comments
            .iter()
            .map(|comment| match comment {
                Comment::Line(token) => (token.span.clone(), true),
                Comment::Block(start, end) => {
                    (start.span.start..end.span.end, false)
                }
            })
            .collect();
        raw.sort_by_key(|(span, _)| span.start);
        // A line comment inside a block comment is captured twice by the
        // parser; only the covering block comment is real.
        let mut covered_end = 0;
        raw.retain(|(span, _)| {
            if span.start < covered_end {
                return false;
            }
            covered_end = span.end;
            true
        });

        let line_of = |byte: usize| {
            line_starts.partition_point(|start| *start <= byte) - 1
        };
        let comments = raw
            .iter()
            .map(|(span, is_line)| MappedComment {
                text: &source[span.clone()],
                span: span.clone(),
                start_line: line_of(span.start),
                end_line: line_of(span.end),
                is_line: *is_line,
                class: CommentClass::OwnLine,
                claimed: false,
            })
            .collect();

        let mut map = Self {
            source,
            line_starts,
            comments,
            misplaced: 0,
        };
        let classes: Vec<CommentClass> = map
            .comments
            .iter()
            .map(|comment| map.classify(comment))
            .collect();
        for (comment, class) in map.comments.iter_mut().zip(classes) {
            comment.class = class;
        }
        map
    }

    pub fn line_of(&self, byte: usize) -> usize {
        self.line_starts.partition_point(|start| *start <= byte) - 1
    }

    /// The comment span covering `byte`, if any.
    fn comment_covering(&self, byte: usize) -> Option<Range<usize>> {
        let idx = self
            .comments
            .partition_point(|comment| comment.span.start <= byte);
        let span = &self.comments.get(idx.checked_sub(1)?)?.span;
        (span.end > byte).then(|| span.clone())
    }

    /// Whether `range` holds only whitespace, comments, and `allowed` bytes.
    fn is_clear(&self, range: Range<usize>, allowed: &[u8]) -> bool {
        let bytes = self.source.as_bytes();
        let mut i = range.start;
        while i < range.end {
            let byte = bytes[i];
            if byte.is_ascii_whitespace() || allowed.contains(&byte) {
                i += 1;
            } else if let Some(span) = self.comment_covering(i) {
                i = span.end;
            } else {
                return false;
            }
        }
        true
    }

    /// The first code byte after `from` on `line`, skipping comments.
    fn next_code_on_line(&self, from: usize, line: usize) -> Option<u8> {
        let bytes = self.source.as_bytes();
        let line_end = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.source.len());
        let mut i = from;
        while i < line_end {
            let byte = bytes[i];
            if byte.is_ascii_whitespace() {
                i += 1;
            } else if let Some(span) = self.comment_covering(i) {
                i = span.end;
            } else {
                return Some(byte);
            }
        }
        None
    }

    fn classify(&self, comment: &MappedComment) -> CommentClass {
        let line_start = self.line_starts[comment.start_line];
        let code_before = !self.is_clear(line_start..comment.span.start, &[]);
        let multi_line = comment.end_line > comment.start_line;
        let after = self.next_code_on_line(comment.span.end, comment.end_line);
        if code_before {
            match after {
                Some(byte) if !comment.is_line && !CLOSERS.contains(&byte) => {
                    CommentClass::InlineLeading
                }
                _ => CommentClass::Trailing,
            }
        } else if !comment.is_line && !multi_line {
            match after {
                Some(byte) if !CLOSERS.contains(&byte) => {
                    CommentClass::InlineLeading
                }
                _ => CommentClass::OwnLine,
            }
        } else {
            CommentClass::OwnLine
        }
    }

    fn to_print(comment: &MappedComment<'source>) -> CommentToPrint<'source> {
        CommentToPrint {
            text: comment.text,
            start_line: comment.start_line,
            end_line: comment.end_line,
            inline: comment.class == CommentClass::InlineLeading,
            is_line: comment.is_line,
        }
    }

    fn claim(
        &mut self,
        mut eligible: impl FnMut(&MappedComment) -> bool,
    ) -> Vec<CommentToPrint<'source>> {
        let mut result = vec![];
        for comment in &mut self.comments {
            if !comment.claimed && eligible(comment) {
                comment.claimed = true;
                result.push(Self::to_print(comment));
            }
        }
        result
    }

    /// Whether an unclaimed leading-position comment is adjacent to the
    /// construct starting at `anchor`: nothing but whitespace and other
    /// comments up to the anchor's line (a construct's span may start
    /// after a keyword, so its own line is not scanned).
    fn leads(&self, comment: &MappedComment, anchor: usize) -> bool {
        let line_start = self.line_starts[self.line_of(anchor)];
        comment.span.end <= anchor
            && self.is_clear(
                comment.span.end..line_start.max(comment.span.end),
                &[],
            )
    }

    fn take_leading_inner(
        &mut self,
        anchor: usize,
        strict: bool,
        inline_only: bool,
    ) -> Vec<CommentToPrint<'source>> {
        let mut result = vec![];
        for idx in 0..self.comments.len() {
            let comment = &self.comments[idx];
            if comment.span.end > anchor {
                break;
            }
            let class_fits = if inline_only {
                comment.class == CommentClass::InlineLeading
            } else {
                comment.class != CommentClass::Trailing
            };
            let adjacent = if strict {
                self.is_clear(comment.span.end..anchor, &[])
            } else {
                self.leads(comment, anchor)
            };
            if comment.claimed || !class_fits || !adjacent {
                continue;
            }
            self.comments[idx].claimed = true;
            result.push(Self::to_print(&self.comments[idx]));
        }
        result
    }

    /// Claims the leading comments of the construct starting at `anchor`:
    /// own-line and inline-leading comments adjacent to it.
    pub fn take_leading(
        &mut self,
        anchor: usize,
    ) -> Vec<CommentToPrint<'source>> {
        self.take_leading_inner(anchor, false, false)
    }

    /// [`Self::take_leading`] under strict byte adjacency (nothing but
    /// whitespace and comments up to `anchor` itself); for expressions,
    /// which start mid-line and must not pull comments across the code
    /// before them.
    pub fn take_adjacent_leading(
        &mut self,
        anchor: usize,
    ) -> Vec<CommentToPrint<'source>> {
        self.take_leading_inner(anchor, true, false)
    }

    /// [`Self::take_adjacent_leading`] restricted to inline-leading
    /// comments; for constructs that must not pull whole-line comments
    /// inward.
    pub fn take_inline_leading(
        &mut self,
        anchor: usize,
    ) -> Vec<CommentToPrint<'source>> {
        self.take_leading_inner(anchor, true, true)
    }

    /// Claims the trailing comments of the construct ending at `end` on
    /// `end_line`: comments on that line after it, separated by nothing but
    /// whitespace, separators, and other comments.
    pub fn take_trailing(
        &mut self,
        end: usize,
        end_line: usize,
    ) -> Vec<CommentToPrint<'source>> {
        let mut result = vec![];
        for idx in 0..self.comments.len() {
            let comment = &self.comments[idx];
            if comment.claimed
                || comment.class != CommentClass::Trailing
                || comment.start_line != end_line
                || comment.span.start < end
                || !self.is_clear(end..comment.span.start, b",;")
            {
                continue;
            }
            self.comments[idx].claimed = true;
            result.push(Self::to_print(&self.comments[idx]));
        }
        result
    }

    /// Claims comments trailing an opening delimiter at `open`: trailing
    /// comments on its line starting before `upper` (the first element).
    pub fn take_open_trailing(
        &mut self,
        open: usize,
        upper: usize,
    ) -> Vec<CommentToPrint<'source>> {
        let line = self.line_of(open);
        self.claim(|comment| {
            comment.class == CommentClass::Trailing
                && comment.start_line == line
                && comment.span.start > open
                && comment.span.start < upper
        })
    }

    /// Claims the own-line comments left in `range`; for scope ends, where
    /// no construct follows them.
    pub fn take_between(
        &mut self,
        range: Range<usize>,
    ) -> Vec<CommentToPrint<'source>> {
        self.claim(|comment| {
            comment.class != CommentClass::Trailing
                && comment.span.start >= range.start
                && comment.span.start < range.end
        })
    }

    /// Claims every comment inside `range`, whatever its class; for spans
    /// whose source prints verbatim (their text is already in the slice)
    /// and for empty groups, where nothing else can anchor them.
    pub fn take_within(
        &mut self,
        range: &Range<usize>,
    ) -> Vec<CommentToPrint<'source>> {
        self.claim(|comment| {
            comment.span.start >= range.start && comment.span.end <= range.end
        })
    }

    /// Whether `range` holds a comment that rules out any flat layout: a
    /// line comment would swallow the rest of the line, and an own-line or
    /// multi-line comment needs its own line(s).
    pub fn forces_break(&self, range: &Range<usize>) -> bool {
        self.comments.iter().any(|comment| {
            comment.span.start >= range.start
                && comment.span.start < range.end
                && (comment.is_line
                    || comment.end_line > comment.start_line
                    || comment.class == CommentClass::OwnLine)
        })
    }

    /// Claims everything still unclaimed, for the end of the root.
    /// Unclaimed comments before `boundary` were missed by an interior
    /// claim site and count as misplaced.
    pub fn take_rest(
        &mut self,
        boundary: usize,
    ) -> Vec<CommentToPrint<'source>> {
        self.misplaced += self
            .comments
            .iter()
            .filter(|comment| !comment.claimed && comment.span.start < boundary)
            .count();
        self.claim(|_| true)
    }

    /// Comments [`Self::take_rest`] found behind an interior claim site;
    /// each is a placement bug (the comment still prints, at the end of
    /// the output).
    pub fn misplaced(&self) -> usize {
        self.misplaced
    }
}
