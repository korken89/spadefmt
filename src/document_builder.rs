// Copyright (C) 2024 Ethan Uppal.
//
// This file is part of spadefmt.
//
// spadefmt is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version. spadefmt is distributed in the hope that it will be
// useful, but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General
// Public License for more details. You should have received a copy of the GNU
// General Public License along with spadefmt. If not, see <https://www.gnu.org/licenses/>.

use std::{cell::RefCell, ops::Range};

use spade_ast as ast;
use spade_ast::token;
use spade_codespan_reporting::files::{Files, SimpleFile};
use spade_common::{
    location_info::{FullSpan, Loc, WithLocation},
    name::{Identifier, Path, PathSegment, Visibility},
};
use spade_diagnostics::{Diagnostic, codespan::Span};

use crate::{
    comment_insertion::{CommentMap, CommentToPrint},
    document::{Document, DocumentIdx, InternedDocumentStore},
};

pub struct DocumentBuilder<'code> {
    indent: isize,
    file: RefCell<Option<&'code SimpleFile<String, String>>>,
    diagnostics: RefCell<Vec<Diagnostic>>,
    inner: RefCell<InternedDocumentStore>,
}

pub trait BuildAsDocument {
    fn build<'code>(
        &self,
        builder: &DocumentBuilder<'code>,
        comments: &mut CommentMap,
    ) -> DocumentIdx;
}

impl BuildAsDocument for Loc<DocumentIdx> {
    fn build(
        &self,
        _builder: &DocumentBuilder,
        _comments: &mut CommentMap,
    ) -> DocumentIdx {
        self.inner
    }
}

macro_rules! can_build {
    ($T:ty: $name:ident) => {
        impl BuildAsDocument for $T {
            fn build(
                &self,
                builder: &DocumentBuilder,
                comments: &mut CommentMap,
            ) -> $crate::document::DocumentIdx {
                builder.$name(self, comments)
            }
        }

        impl BuildAsDocument for Loc<$T> {
            fn build(
                &self,
                builder: &DocumentBuilder,
                comments: &mut CommentMap,
            ) -> $crate::document::DocumentIdx {
                builder.$name(self, comments)
            }
        }
    };
}

can_build!(ast::Item: build_item);
can_build!(Loc<ast::Expression>: build_expression);
can_build!(Loc<ast::TypeExpression>: build_type_expression);
can_build!(Loc<ast::TypeParam>: build_type_param);
can_build!(Loc<ast::TraitSpec>: build_trait_spec);
can_build!(ast::NamedArgument: build_named_argument);
can_build!(ast::NamedTurbofish: build_named_turbofish);
can_build!(Loc<ast::Pattern>: build_pattern);

pub type AstParameter = (
    ast::AttributeList,
    Option<Loc<ast::WireMarker>>,
    Loc<Identifier>,
    Loc<ast::TypeSpec>,
);

can_build!(AstParameter: build_parameter);

/// An array literal element with an optional `'label`.
pub type AstArrayElement = (Option<Loc<Identifier>>, Loc<ast::Expression>);

/// A named argument in a type pattern: `field: pat`, or `field` shorthand.
pub type AstNamedPatternArgument = (Loc<Identifier>, Option<Loc<ast::Pattern>>);

can_build!(AstNamedPatternArgument: build_named_pattern_argument);

can_build!(AstArrayElement: build_array_element);

can_build!(ast::EnumVariant: build_enum_variant);

/// The source byte range a construct occupies; claims and gap checks key
/// off it.
pub trait HasSourceRange {
    fn byte_range(&self) -> Range<usize>;
}

impl HasSourceRange for Span {
    fn byte_range(&self) -> Range<usize> {
        self.start().to_usize()..self.end().to_usize()
    }
}

impl<T> HasSourceRange for Loc<T> {
    fn byte_range(&self) -> Range<usize> {
        self.span.byte_range()
    }
}

impl HasSourceRange for ast::EnumVariant {
    fn byte_range(&self) -> Range<usize> {
        let start = self
            .attributes
            .0
            .first()
            .map(|first| first.span)
            .unwrap_or(self.name.span)
            .byte_range()
            .start;
        let end = self
            .args
            .as_ref()
            .map(|args| args.span)
            .unwrap_or(self.name.span)
            .byte_range()
            .end;
        start..end
    }
}

impl HasSourceRange for ast::NamedArgument {
    fn byte_range(&self) -> Range<usize> {
        match self {
            ast::NamedArgument::Full(name, value) => {
                name.byte_range().start..value.byte_range().end
            }
            ast::NamedArgument::Short(name) => name.byte_range(),
        }
    }
}

impl HasSourceRange for AstParameter {
    fn byte_range(&self) -> Range<usize> {
        let start = self
            .0
            .0
            .first()
            .map(|first| first.span)
            .or_else(|| self.1.as_ref().map(|wire| wire.span))
            .unwrap_or(self.2.span)
            .byte_range()
            .start;
        start..self.3.byte_range().end
    }
}

impl HasSourceRange for AstNamedPatternArgument {
    fn byte_range(&self) -> Range<usize> {
        let end = self
            .1
            .as_ref()
            .map(|pattern| pattern.byte_range())
            .unwrap_or(self.0.byte_range())
            .end;
        self.0.byte_range().start..end
    }
}

impl HasSourceRange for AstArrayElement {
    fn byte_range(&self) -> Range<usize> {
        let start = self
            .0
            .as_ref()
            .map(|label| label.byte_range())
            .unwrap_or(self.1.byte_range())
            .start;
        start..self.1.byte_range().end
    }
}

/// Source spelling per the lexer's token table. `spade_ast`'s `Display`
/// impls target diagnostics and swap the logical/bitwise pairs, so they
/// must not be used for output.
fn binary_operator_str(op: &ast::BinaryOperator) -> &'static str {
    match op {
        ast::BinaryOperator::Add => "+",
        ast::BinaryOperator::Sub => "-",
        ast::BinaryOperator::Mul => "*",
        ast::BinaryOperator::Div => "/",
        ast::BinaryOperator::Mod => "%",
        ast::BinaryOperator::Eq => "==",
        ast::BinaryOperator::Neq => "!=",
        ast::BinaryOperator::Lt => "<",
        ast::BinaryOperator::Gt => ">",
        ast::BinaryOperator::Le => "<=",
        ast::BinaryOperator::Ge => ">=",
        ast::BinaryOperator::LogicalAnd => "&&",
        ast::BinaryOperator::LogicalOr => "||",
        ast::BinaryOperator::LogicalXor => "^^",
        ast::BinaryOperator::LeftShift => "<<",
        ast::BinaryOperator::RightShift => ">>",
        ast::BinaryOperator::ArithmeticRightShift => ">>>",
        ast::BinaryOperator::BitwiseAnd => "&",
        ast::BinaryOperator::BitwiseOr => "|",
        ast::BinaryOperator::BitwiseXor => "^",
        ast::BinaryOperator::WrappingAdd => "+.",
        ast::BinaryOperator::WrappingSub => "-.",
        ast::BinaryOperator::WrappingMul => "*.",
        ast::BinaryOperator::WrappingLeftShift => "<<.",
        ast::BinaryOperator::WrappingRightShift => ">>.",
    }
}

/// See [`binary_operator_str`].
fn unary_operator_str(op: &ast::UnaryOperator) -> &'static str {
    match op {
        ast::UnaryOperator::Sub => "-",
        ast::UnaryOperator::Not => "!",
        ast::UnaryOperator::BitwiseNot => "~",
        ast::UnaryOperator::WrappingSub => "-.",
        ast::UnaryOperator::Dereference => "*",
        ast::UnaryOperator::Reference => "&",
    }
}

/// Dedents the interior lines of a multi-line slice by their common
/// leading spaces. The writer re-indents after every newline it prints, so
/// a verbatim multi-line slice (macro span, block comment) would otherwise
/// gain one indent level per pass; dedented lines pick up the output depth
/// from the writer instead.
fn dedented_raw(slice: &str) -> String {
    let Some((first_line, rest)) = slice.split_once('\n') else {
        return slice.to_string();
    };
    let leading_spaces = |line: &str| -> usize {
        line.len() - line.trim_start_matches(' ').len()
    };
    let dedent = rest
        .split('\n')
        .filter(|line| !line.trim().is_empty())
        .map(leading_spaces)
        .min()
        .unwrap_or(0);
    let mut text = first_line.to_string();
    for line in rest.split('\n') {
        text.push('\n');
        text.push_str(&line[dedent.min(leading_spaces(line))..]);
    }
    text
}

/// Extends `span` back over the construct's attributes and docs; the
/// parser starts every item and statement span at the keyword, after them.
fn attributed_span(attributes: &ast::AttributeList, span: Span) -> Span {
    attributes
        .0
        .first()
        .map(|first| first.span.merge(span))
        .unwrap_or(span)
}

fn type_declaration_attributes(
    type_declaration: &ast::TypeDeclaration,
) -> &ast::AttributeList {
    match &type_declaration.kind {
        ast::TypeDeclKind::Enum(enum_decl) => &enum_decl.attributes,
        ast::TypeDeclKind::Struct(struct_decl) => &struct_decl.attributes,
        ast::TypeDeclKind::Alias(alias) => &alias.attributes,
    }
}

fn span_of_item(item: &ast::Item) -> Span {
    match item {
        spade_ast::Item::Unit(unit) => {
            attributed_span(&unit.head.attributes, unit.span)
        }
        spade_ast::Item::MacroDef(macro_def) => {
            attributed_span(&macro_def.attributes, macro_def.span)
        }
        spade_ast::Item::TraitDef(trait_definition) => {
            attributed_span(&trait_definition.attributes, trait_definition.span)
        }
        spade_ast::Item::Type(ty) => {
            attributed_span(type_declaration_attributes(ty), ty.span)
        }
        spade_ast::Item::ExternalMod(external_module) => {
            attributed_span(&external_module.attributes, external_module.span)
        }
        spade_ast::Item::Module(module) => {
            attributed_span(&module.attributes, module.span)
        }
        spade_ast::Item::Use(attributes, use_) => {
            attributed_span(attributes, use_.span)
        }
        spade_ast::Item::ImplBlock(impl_block) => impl_block.span,
    }
}

fn span_of_statement(statement: &Loc<ast::Statement>) -> Span {
    let attributes = match &**statement {
        ast::Statement::Binding(binding) => Some(&binding.attrs),
        ast::Statement::Register(register) => Some(&register.attributes),
        ast::Statement::Expression(_, attributes) => Some(attributes),
        ast::Statement::Type(type_declaration) => {
            Some(type_declaration_attributes(type_declaration))
        }
        ast::Statement::Label(_)
        | ast::Statement::Declaration(_)
        | ast::Statement::PipelineRegMarker(..)
        | ast::Statement::Set { .. }
        | ast::Statement::Assert(_) => None,
    };
    attributes
        .map(|attributes| attributed_span(attributes, statement.span))
        .unwrap_or(statement.span)
}

impl<'code> DocumentBuilder<'code> {
    pub fn new(indent: isize) -> Self {
        Self {
            indent,
            file: Default::default(),
            diagnostics: Default::default(),
            inner: Default::default(),
        }
    }

    /// Records an "unsupported construct" diagnostic. The built document is
    /// discarded whenever any of these exist, so builders may continue with
    /// placeholder output afterwards.
    fn record_unsupported(&self, span: impl Into<FullSpan>, construct: &str) {
        self.diagnostics.borrow_mut().push(
            Diagnostic::error(
                span,
                format!("spadefmt cannot format {construct} yet"),
            )
            .primary_label("unsupported construct"),
        );
    }

    /// [`Self::record_unsupported`] for expression position.
    fn unsupported(
        &self,
        span: impl Into<FullSpan>,
        construct: &str,
    ) -> DocumentIdx {
        self.record_unsupported(span, construct);
        self.text("")
    }

    fn line_of(&self, byte: usize) -> usize {
        self.file
            .borrow()
            .unwrap()
            .line_index((), byte)
            .expect("byte position was somehow not from the file it came from")
    }

    /// Appends `leading` before a construct on `construct_line`: own-line
    /// comments on their own lines, inline ones followed by a space, with
    /// source blank lines preserved (collapsed to one).
    fn emit_leading(
        &self,
        leading: &[CommentToPrint],
        construct_line: usize,
        list: &mut Vec<DocumentIdx>,
    ) {
        let mut prev_end_line = None;
        let mut last_inline = false;
        for comment in leading {
            if let Some(prev) = prev_end_line
                && prev + 1 < comment.start_line
            {
                list.push(self.newline());
            }
            list.push(self.raw_text(dedented_raw(comment.text)));
            list.push(if comment.inline {
                self.text(" ")
            } else {
                self.newline()
            });
            prev_end_line = Some(comment.end_line);
            last_inline = comment.inline;
        }
        if !last_inline
            && let Some(prev) = prev_end_line
            && prev + 1 < construct_line
        {
            list.push(self.newline());
        }
    }

    /// Appends `trailing` after a construct, space-separated on its line.
    fn emit_trailing(
        &self,
        trailing: &[CommentToPrint],
        list: &mut Vec<DocumentIdx>,
    ) {
        for comment in trailing {
            list.push(self.text(" "));
            list.push(self.raw_text(dedented_raw(comment.text)));
        }
    }

    /// Appends scope-trailing comments (nothing follows them in their
    /// scope), each on its own line after content ending on
    /// `prev_end_line` ([`None`] at the start of the scope).
    fn emit_scope_trailing(
        &self,
        rest: &[CommentToPrint],
        mut prev_end_line: Option<usize>,
        list: &mut Vec<DocumentIdx>,
    ) {
        for comment in rest {
            if let Some(prev) = prev_end_line {
                list.push(self.newline());
                if prev + 1 < comment.start_line {
                    list.push(self.newline());
                }
            }
            list.push(self.raw_text(dedented_raw(comment.text)));
            prev_end_line = Some(comment.end_line);
        }
    }

    fn source(&self, range: Range<usize>) -> &'code str {
        &self.file.borrow().unwrap().source()[range]
    }

    /// An identifier's source text. The lexer strips a raw `r#` prefix
    /// from the name, so only the span keeps it; a loc that does not hold
    /// the name (a synthetic one, or a `'label`/`@label` token whose span
    /// includes the sigil) yields the bare name.
    fn identifier_text(&self, ident: &Loc<Identifier>) -> String {
        let name = ident.inner.to_string();
        let file = self.file.borrow();
        match file.unwrap().source().get(ident.byte_range()) {
            Some(slice)
                if slice == name
                    || slice.strip_prefix("r#") == Some(name.as_str()) =>
            {
                slice.to_string()
            }
            _ => name,
        }
    }

    fn identifier(&self, ident: &Loc<Identifier>) -> DocumentIdx {
        self.text(self.identifier_text(ident))
    }

    fn segment_text(&self, segment: &PathSegment) -> String {
        match segment {
            PathSegment::Named(ident) => self.identifier_text(ident),
            generated => generated.to_string(),
        }
    }

    fn path_text(&self, segments: &[PathSegment]) -> String {
        segments
            .iter()
            .map(|segment| self.segment_text(segment))
            .collect::<Vec<_>>()
            .join("::")
    }

    /// The exact source bytes of `range` as measured text: a token whose
    /// lexeme the AST does not keep (literals). Comments inside the range
    /// are claimed since their text is already part of the slice.
    fn source_text(
        &self,
        range: Range<usize>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        comments.take_within(&range);
        self.text(self.source(range))
    }

    /// A pattern or type literal, whose loc spans the folded sign, any
    /// comments after it, and the token: prints like an expression
    /// literal, the sign and then the comments leading the token.
    fn signed_literal(
        &self,
        range: Range<usize>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let sign = match self.source(range.clone()).as_bytes() {
            [b'-' | b'+', ..] => 1,
            _ => 0,
        };
        let leading = comments.take_within(&range);
        let after = leading
            .iter()
            .map(|comment| comment.end)
            .fold(range.start + sign, usize::max);
        let token_start =
            range.end - self.source(after..range.end).trim_start().len();
        let token = self.with_adjacent_leading(
            leading,
            token_start,
            self.text(self.source(token_start..range.end)),
        );
        if sign == 0 {
            token
        } else {
            self.list([
                self.text(self.source(range.start..range.start + sign)),
                token,
            ])
        }
    }

    /// The exact source bytes of `span`, printed verbatim. Comments inside
    /// the span are claimed since their text is already part of the slice.
    fn raw_source_span(
        &self,
        span: Span,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let range = span.byte_range();
        comments.take_within(&range);
        self.raw_text(dedented_raw(self.source(range)))
    }

    pub fn build_root(
        self,
        root: &Loc<ast::ModuleBody>,
        file: &'code SimpleFile<String, String>,
        comments: &mut CommentMap,
    ) -> (InternedDocumentStore, DocumentIdx, Vec<Diagnostic>) {
        self.file.replace(Some(file));

        let mut list = vec![];

        // `//!` docs are tokens, not comments (the comment map never sees
        // them) and only parse at the top of the body, so printing them
        // there is exact despite their missing spans.
        for (i, doc) in root.documentation.iter().enumerate() {
            if i > 0 {
                list.push(self.newline());
            }
            list.push(self.text(format!("//!{doc}")));
        }
        if !root.documentation.is_empty() && !root.members.is_empty() {
            list.extend([self.newline(), self.newline()]);
        }

        let mut prev_end_line = None;
        let mut last_end_byte = 0;
        for (i, item) in root.members.iter().enumerate() {
            let item_range = span_of_item(item).byte_range();
            let leading = comments.take_leading(item_range.start);
            let item_line = self.line_of(item_range.start);
            let effective_start = leading
                .first()
                .map(|comment| comment.start_line)
                .unwrap_or(item_line);

            if i > 0 {
                list.push(self.newline());
                if prev_end_line
                    .is_some_and(|prev: usize| prev + 1 < effective_start)
                {
                    list.push(self.newline());
                }
            }
            self.emit_leading(&leading, item_line, &mut list);
            list.push(self.build_item(item, comments));

            let end_line = self.line_of(item_range.end);
            let trailing = comments.take_trailing(item_range.end, end_line);
            self.emit_trailing(&trailing, &mut list);
            prev_end_line = Some(
                trailing
                    .last()
                    .map(|comment| comment.end_line)
                    .unwrap_or(end_line),
            );
            last_end_byte = item_range.end;
        }

        let rest = comments.take_rest(last_end_byte);
        self.emit_scope_trailing(&rest, prev_end_line, &mut list);

        let idx = self.trim_list(list);
        (self.inner.take(), idx, self.diagnostics.take())
    }

    pub fn build_item(
        &self,
        item: &ast::Item,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        match item {
            ast::Item::Unit(unit) => self.build_unit(unit, comments),
            // Macro bodies are raw token streams the lexer has already
            // mangled (lexemes, spacing, and comments are unrecoverable
            // from the AST), so the whole construct prints verbatim from
            // its source span.
            ast::Item::MacroDef(macro_def) => {
                let mut list = vec![self.build_attribute_list(
                    &macro_def.attributes,
                    true,
                    comments,
                )];
                list.extend(self.visibility_prefix(&macro_def.visibility));
                list.push(self.raw_source_span(macro_def.span, comments));
                self.list(list)
            }
            ast::Item::TraitDef(trait_definition) => {
                self.build_trait_def(trait_definition, comments)
            }
            ast::Item::Type(type_declaration) => {
                self.build_type_declaration(type_declaration, comments)
            }
            ast::Item::ExternalMod(external_module) => {
                let mut list = vec![self.build_attribute_list(
                    &external_module.attributes,
                    true,
                    comments,
                )];
                list.extend(
                    self.visibility_prefix(&external_module.visibility),
                );
                list.push(self.text(format!(
                    "mod {};",
                    self.identifier_text(&external_module.name)
                )));
                self.list(list)
            }
            ast::Item::Module(module) => self.build_module(module, comments),
            ast::Item::Use(attributes, use_statements) => {
                self.build_use(attributes, use_statements, comments)
            }
            ast::Item::ImplBlock(impl_block) => {
                self.build_impl_block(impl_block, comments)
            }
        }
    }

    pub fn build_unit(
        &self,
        unit: &Loc<ast::Unit>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let mut list = vec![self.build_unit_head(&unit.head, comments)];

        list.push(match &unit.body {
            Some(body) => self
                .list([self.text(" "), self.build_expression(body, comments)]),
            None => self.text(";"),
        });

        self.list(list)
    }

    pub fn build_unit_head(
        &self,
        head: &ast::UnitHead,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let mut list = vec![];

        list.push(self.build_attribute_list(&head.attributes, true, comments));

        if let Some(visibility) = self.visibility_prefix(&head.visibility) {
            list.push(visibility);
        }
        if head.unsafe_token.is_some() {
            list.push(self.text("unsafe "));
        }
        if head.extern_token.is_some() {
            list.push(self.text("extern "));
        }

        list.push(match &*head.unit_kind {
            ast::UnitKind::Function => self.text("fn"),
            ast::UnitKind::Entity => self.text("entity"),
            ast::UnitKind::Pipeline(depth) => self.list([
                self.text("pipeline("),
                self.build_type_expression(depth, comments),
                self.text(")"),
            ]),
        });

        list.push(self.text(format!(" {}", self.identifier_text(&head.name))));

        if let Some(type_params) = &head.type_params {
            list.push(self.group(
                token::TokenKind::Lt.as_str(),
                &type_params.inner,
                token::TokenKind::Comma,
                token::TokenKind::Gt.as_str(),
                &type_params.byte_range(),
                comments,
            ));
        }

        let parameter_list_doc =
            self.build_parameter_list(&head.inputs, comments);
        let parameter_open = self.token(token::TokenKind::OpenParen);
        let parameter_close = self.token(token::TokenKind::CloseParen);

        let output_type_doc = if let Some((_, output_type)) = &head.output_type
        {
            self.list([
                self.text(" -> "),
                self.build_type_spec(output_type, comments),
            ])
        } else {
            self.list([])
        };

        let broken_parameters = self.list([
            parameter_open,
            parameter_list_doc.1,
            parameter_close,
            output_type_doc,
        ]);
        // A `///` doc or a comment renders as a line comment, so a flat
        // layout would swallow everything after it on the line.
        list.push(
            if Self::parameters_have_doc(&head.inputs) || parameter_list_doc.2 {
                broken_parameters
            } else {
                self.try_catch(
                    self.list([
                        parameter_open,
                        parameter_list_doc.0,
                        parameter_close,
                        self.flatten(output_type_doc),
                    ]),
                    self.try_catch(
                        self.list([
                            parameter_open,
                            parameter_list_doc.0,
                            parameter_close,
                            output_type_doc,
                        ]),
                        broken_parameters,
                    ),
                )
            },
        );

        list.push(self.build_where_clauses(&head.where_clauses, comments));

        self.list(list)
    }

    /// ` where ...` with a leading space, or an empty document when there
    /// are no clauses.
    pub fn build_where_clauses(
        &self,
        clauses: &[ast::WhereClause],
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let mut list = vec![];
        for (i, clause) in clauses.iter().enumerate() {
            list.push(self.text(if i == 0 { " where " } else { ", " }));
            match clause {
                ast::WhereClause::TraitBounds { target, traits } => {
                    list.push(self.build_path(target));
                    list.push(self.text(": "));
                    for (j, trait_spec) in traits.iter().enumerate() {
                        if j > 0 {
                            list.push(self.text(" + "));
                        }
                        list.push(self.build_trait_spec(trait_spec, comments));
                    }
                }
                // The grandfathered `N: { expr }` syntax parses to the
                // same AST as `N == expr` and prints as the latter.
                ast::WhereClause::GenericInt {
                    target,
                    kind,
                    expression,
                    if_unsatisfied,
                } => {
                    let operator = match kind {
                        ast::Inequality::Eq => "==",
                        ast::Inequality::Neq => "!=",
                        ast::Inequality::Lt => "<",
                        ast::Inequality::Leq => "<=",
                        ast::Inequality::Gt => ">",
                        ast::Inequality::Geq => ">=",
                    };
                    list.push(self.build_path(target));
                    list.push(self.text(format!(" {operator} ")));
                    list.push(self.build_expression(expression, comments));
                    if let Some(message) = if_unsatisfied {
                        list.push(self.text(" else "));
                        list.push(self.string_literal(message));
                    }
                }
            }
        }
        self.list(list)
    }

    pub fn build_type_declaration(
        &self,
        type_declaration: &Loc<ast::TypeDeclaration>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let visibility = self.visibility_prefix(&type_declaration.visibility);
        match &type_declaration.kind {
            ast::TypeDeclKind::Enum(enum_decl) => {
                let mut list = vec![self.build_attribute_list(
                    &enum_decl.attributes,
                    true,
                    comments,
                )];
                list.extend(visibility);
                list.push(self.text("enum "));
                list.push(self.identifier(&enum_decl.name));
                if let Some(generic_args) = &type_declaration.generic_args {
                    list.push(self.group(
                        token::TokenKind::Lt.as_str(),
                        &generic_args.inner,
                        token::TokenKind::Comma,
                        token::TokenKind::Gt.as_str(),
                        &generic_args.byte_range(),
                        comments,
                    ));
                }
                let options_doc = self.group_raw(
                    &(enum_decl.name.byte_range().end
                        ..type_declaration.byte_range().end),
                    &enum_decl.variants,
                    token::TokenKind::Comma,
                    comments,
                );
                list.extend([
                    self.text(" {"),
                    // self.try_catch(
                    //     self.list([
                    //         self.text(" "),
                    //         options_doc.0,
                    //         self.text(" "),
                    //     ]),
                    options_doc.1,
                    // ),
                    self.text("}"),
                ]);
                self.list(list)
            }
            ast::TypeDeclKind::Struct(struct_decl) => {
                let mut list = vec![self.build_attribute_list(
                    &struct_decl.attributes,
                    true,
                    comments,
                )];
                list.extend(visibility);
                list.push(self.text("struct "));
                list.push(self.identifier(&struct_decl.name));
                if let Some(generic_args) = &type_declaration.generic_args {
                    list.push(self.group(
                        token::TokenKind::Lt.as_str(),
                        &generic_args.inner,
                        token::TokenKind::Comma,
                        token::TokenKind::Gt.as_str(),
                        &generic_args.byte_range(),
                        comments,
                    ));
                }
                let parameter_list_doc =
                    self.build_parameter_list(&struct_decl.members, comments);
                list.extend([
                    self.text(" {"),
                    // self.try_catch(
                    //     self.list([
                    //         self.text(" "),
                    //         parameter_list_doc.0,
                    //         self.text(" "),
                    //     ]),
                    parameter_list_doc.1,
                    // ),
                    self.text("}"),
                ]);
                self.list(list)
            }
            // The alias parser eats its own `;`, so the printed one
            // belongs here in both item and statement position.
            ast::TypeDeclKind::Alias(alias) => {
                let mut list = vec![self.build_attribute_list(
                    &alias.attributes,
                    true,
                    comments,
                )];
                list.extend(visibility);
                list.push(self.text("type "));
                list.push(self.identifier(&alias.name));
                if let Some(generic_args) = &type_declaration.generic_args {
                    list.push(self.group(
                        token::TokenKind::Lt.as_str(),
                        &generic_args.inner,
                        token::TokenKind::Comma,
                        token::TokenKind::Gt.as_str(),
                        &generic_args.byte_range(),
                        comments,
                    ));
                }
                list.extend([
                    self.text(" = "),
                    self.build_type_spec(&alias.type_spec, comments),
                    self.token(token::TokenKind::Semi),
                ]);
                self.list(list)
            }
        }
    }

    pub fn build_enum_variant(
        &self,
        variant: &ast::EnumVariant,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        // Attribute and doc lines are safe here only because the enum arm
        // always breaks its variant list; restoring the commented-out
        // flatten try_catch in `build_type_declaration` would need a
        // docs-present check on variants.
        let mut list = vec![self.build_attribute_list(
            &variant.attributes,
            true,
            comments,
        )];
        list.push(self.identifier(&variant.name));
        if let Some(parameter_list) = &variant.args {
            let parameter_list_doc =
                self.build_parameter_list(parameter_list, comments);
            list.push(self.text(" {"));
            if !self.is_empty(parameter_list_doc.1) {
                list.push(
                    if Self::parameters_have_doc(parameter_list)
                        || parameter_list_doc.2
                    {
                        parameter_list_doc.1
                    } else {
                        self.try_catch(
                            self.list([
                                self.text(" "),
                                parameter_list_doc.0,
                                self.text(" "),
                            ]),
                            parameter_list_doc.1,
                        )
                    },
                );
            }
            list.push(self.text("}"));
        }
        self.list(list)
    }

    pub fn build_module(
        &self,
        item: &Loc<ast::Module>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let mut list =
            vec![self.build_attribute_list(&item.attributes, true, comments)];
        list.extend(self.visibility_prefix(&item.visibility));
        list.push(
            self.text(format!("mod {} {{", self.identifier_text(&item.name))),
        );
        let body =
            self.build_module_body(&item.body, item.byte_range().end, comments);
        if !self.is_empty(body) {
            list.extend([
                self.newline(),
                self.nest(body, self.indent),
                self.newline(),
            ]);
        }
        list.push(self.text("}"));
        self.list(list)
    }

    /// `end` bounds the scope-trailing comment claim (the enclosing
    /// module's closing brace).
    pub fn build_module_body(
        &self,
        body: &Loc<ast::ModuleBody>,
        end: usize,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let mut list = vec![];

        // See `build_root` for why printing `//!` docs first is exact.
        for (i, doc) in body.documentation.iter().enumerate() {
            if i > 0 {
                list.push(self.newline());
            }
            list.push(self.text(format!("//!{doc}")));
        }
        if !body.documentation.is_empty() && !body.members.is_empty() {
            list.extend([self.newline(), self.newline()]);
        }

        let mut prev_end_line = None;
        let mut last_end_byte = body.byte_range().start;
        for (i, item) in body.members.iter().enumerate() {
            let item_range = span_of_item(item).byte_range();
            let leading = comments.take_leading(item_range.start);
            let item_line = self.line_of(item_range.start);
            let effective_start = leading
                .first()
                .map(|comment| comment.start_line)
                .unwrap_or(item_line);

            if i > 0 {
                list.push(self.newline());
                if prev_end_line
                    .is_some_and(|prev: usize| prev + 1 < effective_start)
                {
                    list.push(self.newline());
                }
            }
            self.emit_leading(&leading, item_line, &mut list);
            list.push(self.build_item(item, comments));

            let end_line = self.line_of(item_range.end);
            let trailing = comments.take_trailing(item_range.end, end_line);
            self.emit_trailing(&trailing, &mut list);
            prev_end_line = Some(
                trailing
                    .last()
                    .map(|comment| comment.end_line)
                    .unwrap_or(end_line),
            );
            last_end_byte = item_range.end;
        }

        let rest = comments.take_between(last_end_byte..end);
        self.emit_scope_trailing(&rest, prev_end_line, &mut list);

        self.list(list)
    }

    pub fn build_use(
        &self,
        attributes: &ast::AttributeList,
        use_statements: &Loc<Vec<ast::UseStatement>>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        if let [use_statement] = use_statements.inner.as_slice() {
            let ast::UseStatement {
                visibility,
                path,
                alias,
            } = use_statement;

            let mut line =
                vec![self.build_attribute_list(attributes, true, comments)];
            line.extend(self.visibility_prefix(visibility));
            line.extend([self.text("use "), self.build_path(path)]);

            if let Some(alias) = alias {
                line.push(
                    self.text(format!(" as {}", self.identifier_text(alias))),
                );
            }

            line.push(self.text(";"));
            return self.list(line);
        }

        // The parser flattens the brace tree of `use a::{b, c::d};` into
        // one statement per leaf; reconstruct one-level braces over the
        // longest common path prefix, capped so every statement keeps at
        // least one segment. Visibility is uniform across the group.
        let statements = &use_statements.inner;
        // `use a::{};` has no leaf, so the AST keeps nothing of its path.
        let Some(first) = statements.first() else {
            return self.unsupported(use_statements, "an empty `use` list");
        };
        let mut prefix_len = statements
            .iter()
            .map(|statement| statement.path.0.len() - 1)
            .min()
            .unwrap_or(0);
        for statement in &statements[1..] {
            let common = first
                .path
                .0
                .iter()
                .zip(statement.path.0.iter())
                .take_while(|(a, b)| {
                    self.segment_text(a) == self.segment_text(b)
                })
                .count();
            prefix_len = prefix_len.min(common);
        }

        let mut line =
            vec![self.build_attribute_list(attributes, true, comments)];
        line.extend(self.visibility_prefix(&first.visibility));
        line.push(self.text("use "));
        if prefix_len > 0 {
            line.push(self.text(format!(
                "{}::",
                self.path_text(&first.path.0[..prefix_len])
            )));
        }
        let entries = statements
            .iter()
            .map(|statement| {
                let mut entry = self.path_text(&statement.path.0[prefix_len..]);
                if let Some(alias) = &statement.alias {
                    entry.push_str(&format!(
                        " as {}",
                        self.identifier_text(alias)
                    ));
                }
                let doc = self.text(entry);
                match &statement.alias {
                    Some(alias) => doc.between_locs(&statement.path, alias),
                    None => doc.at_loc(&statement.path),
                }
            })
            .collect::<Vec<_>>();
        line.push(self.group(
            token::TokenKind::OpenBrace.as_str(),
            &entries,
            token::TokenKind::Comma,
            token::TokenKind::CloseBrace.as_str(),
            &use_statements.byte_range(),
            comments,
        ));
        line.push(self.text(";"));
        self.list(line)
    }

    pub fn build_impl_block(
        &self,
        impl_block: &Loc<ast::ImplBlock>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let mut list = vec![self.text("impl")];
        if let Some(type_params) = &impl_block.type_params {
            list.push(self.group(
                token::TokenKind::Lt.as_str(),
                &type_params.inner,
                token::TokenKind::Comma,
                token::TokenKind::Gt.as_str(),
                &type_params.byte_range(),
                comments,
            ));
        }
        list.push(self.text(" "));
        if let Some(impl_trait) = &impl_block.r#trait {
            list.extend([
                self.build_trait_spec(impl_trait, comments),
                self.text(" for "),
            ]);
        }
        list.push(self.build_type_spec(&impl_block.target, comments));
        list.push(
            self.build_where_clauses(&impl_block.where_clauses, comments),
        );

        list.push(self.text(" {"));
        let mut member_list = vec![];
        let mut prev_end_line = None;
        let mut last_end_byte = impl_block.target.byte_range().end;
        for assoc_type in &impl_block.assoc_types {
            let range = attributed_span(
                type_declaration_attributes(assoc_type),
                assoc_type.span,
            )
            .byte_range();
            self.begin_member(
                &range,
                prev_end_line,
                comments,
                &mut member_list,
            );
            member_list.push(self.build_type_declaration(assoc_type, comments));
            prev_end_line =
                Some(self.end_member(&range, comments, &mut member_list));
            last_end_byte = last_end_byte.max(range.end);
        }
        for unit in &impl_block.units {
            let range =
                attributed_span(&unit.head.attributes, unit.span).byte_range();
            self.begin_member(
                &range,
                prev_end_line,
                comments,
                &mut member_list,
            );
            member_list.push(self.build_unit(unit, comments));
            prev_end_line =
                Some(self.end_member(&range, comments, &mut member_list));
            last_end_byte = last_end_byte.max(range.end);
        }
        let rest =
            comments.take_between(last_end_byte..impl_block.byte_range().end);
        self.emit_scope_trailing(&rest, prev_end_line, &mut member_list);
        self.push_body(member_list, &mut list);
        list.push(self.text("}"));

        self.list(list)
    }

    /// Appends an impl or trait body between its braces, or nothing when
    /// it has no members and no comments (`{}`).
    fn push_body(
        &self,
        member_list: Vec<DocumentIdx>,
        list: &mut Vec<DocumentIdx>,
    ) {
        if member_list.is_empty() {
            return;
        }
        list.extend([
            self.newline(),
            self.nest(self.list(member_list), self.indent),
            self.newline(),
        ]);
    }

    /// Separators, blank-line gap, and leading comments before an impl or
    /// trait member spanning `range`.
    fn begin_member(
        &self,
        range: &Range<usize>,
        prev_end_line: Option<usize>,
        comments: &mut CommentMap,
        member_list: &mut Vec<DocumentIdx>,
    ) {
        let leading = comments.take_leading(range.start);
        let member_line = self.line_of(range.start);
        let effective_start = leading
            .first()
            .map(|comment| comment.start_line)
            .unwrap_or(member_line);
        if let Some(prev) = prev_end_line {
            member_list.push(self.newline());
            if prev + 1 < effective_start {
                member_list.push(self.newline());
            }
        }
        self.emit_leading(&leading, member_line, member_list);
    }

    /// Trailing comments of a member spanning `range`; returns its
    /// effective end line.
    fn end_member(
        &self,
        range: &Range<usize>,
        comments: &mut CommentMap,
        member_list: &mut Vec<DocumentIdx>,
    ) -> usize {
        let end_line = self.line_of(range.end);
        let trailing = comments.take_trailing(range.end, end_line);
        self.emit_trailing(&trailing, member_list);
        trailing
            .last()
            .map(|comment| comment.end_line)
            .unwrap_or(end_line)
    }

    pub fn build_trait_def(
        &self,
        trait_def: &Loc<ast::TraitDef>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let mut list = vec![self.build_attribute_list(
            &trait_def.attributes,
            true,
            comments,
        )];
        list.extend(self.visibility_prefix(&trait_def.visibility));
        list.push(
            self.text(format!(
                "trait {}",
                self.identifier_text(&trait_def.name)
            )),
        );
        if let Some(type_params) = &trait_def.type_params {
            list.push(self.group(
                token::TokenKind::Lt.as_str(),
                &type_params.inner,
                token::TokenKind::Comma,
                token::TokenKind::Gt.as_str(),
                &type_params.byte_range(),
                comments,
            ));
        }
        for (i, subtrait) in trait_def.subtraits.iter().enumerate() {
            list.push(self.text(if i == 0 { ": " } else { " + " }));
            list.push(self.build_trait_spec(subtrait, comments));
        }
        list.push(self.build_where_clauses(&trait_def.where_clauses, comments));

        list.push(self.text(" {"));
        let mut member_list = vec![];
        let mut prev_end_line = None;
        let mut last_end_byte = trait_def.name.byte_range().end;
        // The AST stores associated types and methods separately, so
        // source interleaving is lost; associated types print first.
        for assoc_type in &trait_def.assoc_types {
            let range = assoc_type.byte_range();
            self.begin_member(
                &range,
                prev_end_line,
                comments,
                &mut member_list,
            );
            member_list.push(self.text(format!(
                "type {}",
                self.identifier_text(&assoc_type.name)
            )));
            if let Some(type_params) = &assoc_type.type_params {
                member_list.push(self.group(
                    token::TokenKind::Lt.as_str(),
                    &type_params.inner,
                    token::TokenKind::Comma,
                    token::TokenKind::Gt.as_str(),
                    &type_params.byte_range(),
                    comments,
                ));
            }
            member_list.push(self.token(token::TokenKind::Semi));
            prev_end_line =
                Some(self.end_member(&range, comments, &mut member_list));
            last_end_byte = last_end_byte.max(range.end);
        }
        for method in &trait_def.methods {
            let range =
                attributed_span(&method.attributes, method.span).byte_range();
            self.begin_member(
                &range,
                prev_end_line,
                comments,
                &mut member_list,
            );
            member_list.push(self.build_unit_head(method, comments));
            member_list.push(self.token(token::TokenKind::Semi));
            prev_end_line =
                Some(self.end_member(&range, comments, &mut member_list));
            last_end_byte = last_end_byte.max(range.end);
        }
        let rest =
            comments.take_between(last_end_byte..trait_def.byte_range().end);
        self.emit_scope_trailing(&rest, prev_end_line, &mut member_list);
        self.push_body(member_list, &mut list);
        list.push(self.text("}"));

        self.list(list)
    }

    pub fn build_path(&self, path: &Loc<Path>) -> DocumentIdx {
        self.text(self.path_text(&path.inner.0))
    }

    pub fn build_statement(
        &self,
        statement: &Loc<ast::Statement>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let (mut list, wants_semi) = match &**statement {
            ast::Statement::Label(name) => (
                vec![self.text(format!("'{}", self.identifier_text(name)))],
                false,
            ),
            ast::Statement::Declaration(names) => (
                vec![self.text(format!(
                    "decl {}",
                    names
                        .iter()
                        .map(|name| self.identifier_text(name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))],
                true,
            ),
            ast::Statement::Binding(binding) => {
                let mut list = vec![
                    self.build_attribute_list(&binding.attrs, true, comments),
                    self.text("let "),
                    self.build_pattern(&binding.pattern, comments),
                ];

                if let Some(ty) = &binding.ty {
                    list.extend([
                        self.text(": "),
                        self.build_type_spec(ty, comments),
                    ]);
                }

                list.push(self.text(" = "));
                list.push(self.build_expression(&binding.value, comments));

                (list, true)
            }
            ast::Statement::PipelineRegMarker(count, condition) => {
                let mut list = vec![self.text("reg")];

                if let Some(condition) = condition {
                    list.extend([
                        self.token(token::TokenKind::OpenBracket),
                        self.build_expression(condition, comments),
                        self.token(token::TokenKind::CloseBracket),
                    ]);
                }

                if let Some(count) = count {
                    list.extend([
                        self.text(" * "),
                        self.build_type_expression(count, comments),
                    ]);
                }

                (list, true)
            }
            ast::Statement::Register(register) => {
                let mut list = vec![
                    self.build_attribute_list(
                        &register.attributes,
                        true,
                        comments,
                    ),
                    self.text("reg("),
                    self.build_expression(&register.clock, comments),
                    self.text(") "),
                    self.build_pattern(&register.pattern, comments),
                ];

                if let Some(value_type) = &register.value_type {
                    list.extend([
                        self.text(": "),
                        self.build_type_spec(value_type, comments),
                    ]);
                }

                list.push(self.text(" "));

                if let Some(reset) = &register.reset {
                    list.extend([
                        self.text("reset("),
                        self.build_expression(&reset.0, comments),
                        self.text(": "),
                        self.build_expression(&reset.1, comments),
                        self.text(") "),
                    ]);
                }

                if let Some(initial) = &register.initial {
                    list.extend([
                        self.text("initial("),
                        self.build_expression(&initial, comments),
                        self.text(") "),
                    ]);
                }

                list.extend([
                    self.text("= "),
                    self.build_expression(&register.value, comments),
                ]);

                (list, true)
            }
            ast::Statement::Set { target, value } => (
                vec![
                    self.text("set "),
                    self.build_expression(target, comments),
                    self.text(" = "),
                    self.build_expression(value, comments),
                ],
                true,
            ),
            ast::Statement::Assert(expression) => (
                vec![
                    self.text("assert "),
                    self.build_expression(expression, comments),
                ],
                true,
            ),
            ast::Statement::Expression(expression, attributes) => (
                vec![
                    self.build_attribute_list(attributes, true, comments),
                    self.build_expression(expression, comments),
                ],
                true,
            ),
            // Enum and struct take no semicolon; an alias owns its own.
            ast::Statement::Type(type_declaration) => (
                vec![self.build_type_declaration(type_declaration, comments)],
                false,
            ),
        };
        if wants_semi {
            list.push(self.token(token::TokenKind::Semi));
        }

        self.list(list)
    }

    pub fn build_expression(
        &self,
        expression: &Loc<ast::Expression>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        // Comments directly above or inline before an expression that no
        // coarser construct claimed (e.g. above a binding's value) belong
        // to it.
        let start = expression.byte_range().start;
        let leading = comments.take_adjacent_leading(start);
        let doc = self.build_expression_inner(expression, comments);
        self.with_adjacent_leading(leading, start, doc)
    }

    /// Prefixes the construct `doc` starting at `start` with its claimed
    /// adjacent leading comments. Own-line ones break the line before and
    /// after and nest the comment and construct one level as a
    /// continuation.
    fn with_adjacent_leading(
        &self,
        leading: Vec<CommentToPrint>,
        start: usize,
        doc: DocumentIdx,
    ) -> DocumentIdx {
        if leading.is_empty() {
            return doc;
        }
        let own_line = leading.iter().any(|comment| !comment.inline);
        let mut list = vec![];
        if own_line {
            list.push(self.newline());
        }
        self.emit_leading(&leading, self.line_of(start), &mut list);
        list.push(doc);
        if own_line {
            self.nest(self.list(list), self.indent)
        } else {
            self.list(list)
        }
    }

    fn build_expression_inner(
        &self,
        expression: &Loc<ast::Expression>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        match &**expression {
            ast::Expression::Identifier(path) => self.build_path(path),
            // The parser folds a unary `-` into the literal's value and
            // keeps only the bare token in the inner loc (`--5` is 5,
            // `-0` is 0), so the sign comes from the value and comments
            // between the sign(s) and the token lead the token like an
            // operand's would.
            ast::Expression::IntLiteral(int_literal) => {
                let range = int_literal.byte_range();
                let leading = comments
                    .take_within(&(expression.byte_range().start..range.start));
                let token = self.with_adjacent_leading(
                    leading,
                    range.start,
                    self.source_text(range, comments),
                );
                if int_literal.is_negative() {
                    self.list([self.text("-"), token])
                } else {
                    token
                }
            }
            ast::Expression::BoolLiteral(bool_literal) => {
                self.text(bool_literal.to_string())
            }
            ast::Expression::TriLiteral(bit_literal) => {
                self.text(match **bit_literal {
                    ast::BitLiteral::Low => "LOW",
                    ast::BitLiteral::High => "HIGH",
                    ast::BitLiteral::HighImp => "HIGHIMP",
                })
            }
            // The parser desugars `b"ab"` into an array of `uint<8>`
            // literals, one per character inside the quotes; a real array
            // span starts with `[`.
            ast::Expression::ArrayLiteral(elements) => {
                let range = expression.byte_range();
                if self.source(range.clone()).starts_with("b\"") {
                    comments.take_within(&range);
                    self.verbatim(self.source(range))
                } else {
                    self.group(
                        token::TokenKind::OpenBracket.as_str(),
                        elements,
                        token::TokenKind::Comma,
                        token::TokenKind::CloseBracket.as_str(),
                        &range,
                        comments,
                    )
                }
            }
            ast::Expression::ArrayShorthandLiteral(element, amount) => self
                .list([
                    self.token(token::TokenKind::OpenBracket),
                    self.build_expression(element, comments),
                    self.token(token::TokenKind::Semi),
                    self.text(" "),
                    self.build_expression(amount, comments),
                    self.token(token::TokenKind::CloseBracket),
                ]),
            ast::Expression::Index(target, index) => self.list([
                self.build_expression(target, comments),
                self.token(token::TokenKind::OpenBracket),
                self.build_expression(index, comments),
                self.token(token::TokenKind::CloseBracket),
            ]),
            ast::Expression::RangeIndex { target, start, end } => {
                let mut list = vec![
                    self.build_expression(target, comments),
                    self.token(token::TokenKind::OpenBracket),
                ];
                if let Some(start) = start {
                    list.push(self.build_expression(start, comments));
                }
                list.push(self.token(token::TokenKind::DotDot));
                if let Some(end) = end {
                    list.push(self.build_expression(end, comments));
                }
                list.push(self.token(token::TokenKind::CloseBracket));
                self.list(list)
            }
            ast::Expression::TupleLiteral(items) => self.group(
                token::TokenKind::OpenParen.as_str(),
                items,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                &expression.byte_range(),
                comments,
            ),
            // The deprecated `x#0` syntax normalizes to `x.0`.
            ast::Expression::TupleIndex { target, index, .. } => self.list([
                self.build_expression(target, comments),
                self.text("."),
                self.source_text(index.byte_range(), comments),
            ]),
            ast::Expression::FieldAccess(parent, field) => self.list([
                self.build_expression(parent, comments),
                self.text(format!(".{}", self.identifier_text(field))),
            ]),
            ast::Expression::TypeCast(target, ty) => self.list([
                self.build_expression(target, comments),
                self.text(" as "),
                self.build_type_expression(ty, comments),
            ]),
            ast::Expression::LabelAccess { label, field } => self.list([
                self.text("@"),
                self.build_path(label),
                self.text(format!(".{}", self.identifier_text(field))),
            ]),
            // Verbatim like `Item::MacroDef`; the span runs from the
            // callee path through the closing delimiter, whose kind only
            // the source records.
            ast::Expression::MacroCall { .. } => {
                self.raw_source_span(expression.span, comments)
            }
            // The parser recovered around a malformed expression and
            // embedded the real diagnostic in the node instead of pushing
            // it to its diagnostic list, so `format.rs`'s failed-parse
            // gate never saw it.
            ast::Expression::Incomplete(diagnostic, _) => {
                self.diagnostics.borrow_mut().push(diagnostic.clone());
                self.text("")
            }
            ast::Expression::Call {
                kind,
                callee,
                args,
                turbofish,
            } => {
                let mut list = match kind {
                    ast::CallKind::Function => vec![],
                    ast::CallKind::Entity(_) => vec![self.text("inst ")],
                    ast::CallKind::Pipeline(_, latency) => vec![
                        self.text("inst("),
                        self.build_type_expression(latency, comments),
                        self.text(") "),
                    ],
                };

                list.push(self.build_path(callee));
                if let Some(turbofish) = turbofish {
                    list.push(self.build_turbofish(turbofish, comments));
                }
                list.push(self.build_argument_list(args, comments));

                self.list(list)
            }
            ast::Expression::MethodCall {
                target,
                name,
                args,
                kind,
                turbofish,
            } => {
                let mut list = vec![
                    self.build_expression(target, comments),
                    self.token(token::TokenKind::Dot),
                ];
                list.extend(match kind {
                    ast::CallKind::Function => vec![],
                    ast::CallKind::Entity(_) => vec![self.text("inst ")],
                    ast::CallKind::Pipeline(_, latency) => vec![
                        self.text("inst("),
                        self.build_type_expression(latency, comments),
                        self.text(") "),
                    ],
                });

                list.push(self.identifier(name));

                if let Some(turbofish) = turbofish {
                    list.push(self.build_turbofish(turbofish, comments))
                }

                list.push(self.build_argument_list(args, comments));

                self.list(list)
            }
            ast::Expression::If {
                cond,
                on_true,
                on_false,
            } => self.list([
                self.text("if "),
                self.build_expression(cond, comments),
                self.text(" "),
                self.build_expression(on_true, comments),
                self.text(" else "),
                self.build_expression(on_false, comments),
            ]),
            ast::Expression::Match {
                expression: against,
                branches: arms,
                if_let,
            } => {
                // `if let` parses to a two-branch `Match` whose second
                // pattern is a synthetic `_`; reconstruct the original.
                if *if_let {
                    if let [(pattern, None, on_true), (_, None, on_false)] =
                        &arms.inner[..]
                    {
                        return self.list([
                            self.text("if let "),
                            self.build_pattern(pattern, comments),
                            self.text(" = "),
                            self.build_expression(against, comments),
                            self.text(" "),
                            self.build_expression(on_true, comments),
                            self.text(" else "),
                            self.build_expression(on_false, comments),
                        ]);
                    }
                    // Unreachable from the parser.
                    return self
                        .unsupported(expression, "this `if let` expression");
                }
                let mut list = vec![
                    self.text("match "),
                    self.build_expression(against, comments),
                ];
                let mut arm_list = vec![];
                for arm in &arms.inner {
                    let mut pattern_list =
                        vec![self.build_pattern(&arm.0, comments)];
                    if let Some(guard) = &arm.1 {
                        pattern_list.extend([
                            self.text(" if "),
                            self.build_expression(guard, comments),
                        ]);
                    }
                    let pattern = self.list(pattern_list);
                    let case = self.list([
                        self.text(format!(
                            " {} ",
                            token::TokenKind::FatArrow.as_str()
                        )),
                        self.build_expression(&arm.2, comments),
                    ]);
                    arm_list.push(
                        self.try_catch(
                            self.list([
                                self.flatten(pattern),
                                self.flatten(case),
                            ]),
                            self.try_catch(
                                self.list([self.flatten(pattern), case]),
                                self.list([pattern, case]),
                            ),
                        )
                        .between_locs(&arm.0, &arm.2),
                    );
                }

                let arms_doc = self.group_raw(
                    &arms.byte_range(),
                    &arm_list,
                    token::TokenKind::Comma,
                    comments,
                );
                list.push(self.text(" {"));
                if !self.is_empty(arms_doc.1) {
                    list.push(if arms_doc.2 {
                        arms_doc.1
                    } else {
                        self.try_catch(
                            self.list([
                                self.text(" "),
                                arms_doc.0,
                                self.text(" "),
                            ]),
                            arms_doc.1,
                        )
                    });
                }
                list.push(self.text("}"));
                self.list(list)
            }
            // TODO: proper parenthesization in both of these
            ast::Expression::UnaryOperator(unary_operator, inner) => {
                self.list([
                    self.text(unary_operator_str(unary_operator)),
                    self.build_expression(inner, comments),
                ])
            }
            ast::Expression::BinaryOperator(left, op, right) => self.list([
                self.build_expression(left, comments),
                self.text(format!(" {} ", binary_operator_str(op))),
                self.build_expression(right, comments),
            ]),
            ast::Expression::Block(block) => {
                self.build_block(block, expression.byte_range(), comments)
            }
            ast::Expression::PipelineReference { stage, name, .. } => {
                let stage_doc = match stage {
                    ast::PipelineStageReference::Absolute(identifier) => {
                        self.identifier(identifier)
                    }
                    // The parser eats the mandatory sign; `-` survives
                    // as a synthetic outer negation.
                    ast::PipelineStageReference::Relative(offset) => {
                        match &**offset {
                            ast::TypeExpression::ConstGeneric(offset) => {
                                match &***offset {
                                    ast::Expression::UnaryOperator(
                                        operator,
                                        inner,
                                    ) if matches!(
                                        **operator,
                                        ast::UnaryOperator::Sub
                                    ) =>
                                    {
                                        self.list([
                                            self.text("-"),
                                            self.build_expression(
                                                inner, comments,
                                            ),
                                        ])
                                    }
                                    _ => self.list([
                                        self.text("+"),
                                        self.build_expression(offset, comments),
                                    ]),
                                }
                            }
                            _ => self.unsupported(
                                offset,
                                "this pipeline stage reference",
                            ),
                        }
                    }
                };
                self.list([
                    self.text("stage("),
                    stage_doc,
                    self.text(format!(").{}", self.identifier_text(name))),
                ])
            }
            ast::Expression::TypeLevelIf {
                cond,
                on_true,
                on_false,
            } => self.list([
                self.text("gen "),
                self.build_gen_if(cond, on_true, on_false, comments),
            ]),
            ast::Expression::StageValid => self.text("stage.valid"),
            ast::Expression::StageReady => self.text("stage.ready"),
            ast::Expression::StrLiteral(value) => self.string_literal(value),
            ast::Expression::Parenthesized(inner) => self.list([
                self.token(token::TokenKind::OpenParen),
                self.build_expression(inner, comments),
                self.token(token::TokenKind::CloseParen),
            ]),
            ast::Expression::Lambda {
                unit_kind,
                args,
                body,
            } => {
                let mut list = vec![match &**unit_kind {
                    ast::UnitKind::Function => self.text("fn "),
                    ast::UnitKind::Entity => self.text("entity "),
                    ast::UnitKind::Pipeline(depth) => self.list([
                        self.text("pipeline("),
                        self.build_type_expression(depth, comments),
                        self.text(") "),
                    ]),
                }];
                if args.is_empty() {
                    list.push(self.text("||"));
                } else {
                    list.push(self.group(
                        "|",
                        &args.inner,
                        token::TokenKind::Comma,
                        "|",
                        &args.byte_range(),
                        comments,
                    ));
                }
                list.push(self.text(" "));
                // A bare-expression body parses as a statement-less
                // block; print it back bare.
                match (&body.statements[..], &body.result) {
                    ([], Some(result)) => {
                        list.push(self.build_expression(result, comments))
                    }
                    _ => list.push(self.build_block(
                        body,
                        body.byte_range(),
                        comments,
                    )),
                }
                self.list(list)
            }
            ast::Expression::Unsafe(block) => self.list([
                self.text("unsafe "),
                self.build_block(block, block.byte_range(), comments),
            ]),
            ast::Expression::StaticUnreachable(_) => {
                self.unsupported(expression, "`static_unreachable!`")
            }
        }
    }

    /// A `gen if` chain without the leading `gen` keyword, which is only
    /// valid at the head of the chain (`else if`, not `else gen if`).
    fn build_gen_if(
        &self,
        cond: &Loc<ast::Expression>,
        on_true: &Loc<ast::Expression>,
        on_false: &Loc<ast::Expression>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let mut list = vec![
            self.text("if "),
            self.build_expression(cond, comments),
            self.text(" "),
            self.build_expression(on_true, comments),
        ];
        // A missing `else` is stored as a synthetic empty block;
        // `else {}` is dropped either way.
        let empty_else = matches!(
            &**on_false,
            ast::Expression::Block(block)
                if block.statements.is_empty() && block.result.is_none()
        );
        if !empty_else {
            list.push(self.text(" else "));
            list.push(match &**on_false {
                ast::Expression::TypeLevelIf {
                    cond,
                    on_true,
                    on_false,
                } => self.build_gen_if(cond, on_true, on_false, comments),
                _ => self.build_expression(on_false, comments),
            });
        }
        self.list(list)
    }

    /// The braces and contents of a block, with blank-line gaps and
    /// comments preserved; the line indices come from the span the block
    /// was found at.
    pub fn build_block(
        &self,
        block: &ast::Block,
        span_range: Range<usize>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let first_content = block
            .statements
            .first()
            .map(|statement| span_of_statement(statement).byte_range().start)
            .or_else(|| {
                block
                    .result
                    .as_ref()
                    .map(|result| result.byte_range().start)
            });
        let open_trailing = comments.take_open_trailing(
            span_range.start,
            first_content.unwrap_or(span_range.end),
        );

        let mut list = vec![self.token(token::TokenKind::OpenBrace)];
        self.emit_trailing(&open_trailing, &mut list);

        let mut nest = vec![];
        let mut prev_end_line: Option<usize> = None;
        let mut last_end_byte = span_range.start;

        for (i, statement) in block.statements.iter().enumerate() {
            let range = span_of_statement(statement).byte_range();
            let leading = comments.take_leading(range.start);
            let line = self.line_of(range.start);
            let effective_start = leading
                .first()
                .map(|comment| comment.start_line)
                .unwrap_or(line);
            if i > 0
                && prev_end_line.is_some_and(|prev| prev + 1 < effective_start)
            {
                nest.push(self.newline());
            }
            self.emit_leading(&leading, line, &mut nest);
            nest.push(self.build_statement(statement, comments));
            let end_line = self.line_of(range.end);
            let trailing = comments.take_trailing(range.end, end_line);
            self.emit_trailing(&trailing, &mut nest);
            nest.push(self.newline());
            prev_end_line = Some(
                trailing
                    .last()
                    .map(|comment| comment.end_line)
                    .unwrap_or(end_line),
            );
            last_end_byte = range.end;
        }

        if let Some(result) = &block.result {
            let range = result.byte_range();
            let leading = comments.take_leading(range.start);
            let line = self.line_of(range.start);
            let effective_start = leading
                .first()
                .map(|comment| comment.start_line)
                .unwrap_or(line);
            if prev_end_line.is_some_and(|prev| prev + 1 < effective_start) {
                nest.push(self.newline());
            }
            self.emit_leading(&leading, line, &mut nest);
            nest.push(self.build_expression(result, comments));
            let end_line = self.line_of(range.end);
            let trailing = comments.take_trailing(range.end, end_line);
            self.emit_trailing(&trailing, &mut nest);
            nest.push(self.newline());
            prev_end_line = Some(
                trailing
                    .last()
                    .map(|comment| comment.end_line)
                    .unwrap_or(end_line),
            );
            last_end_byte = range.end;
        }

        // Scope-trailing comments before the closing brace.
        let rest = comments.take_between(last_end_byte..span_range.end);
        for comment in &rest {
            if let Some(prev) = prev_end_line
                && prev + 1 < comment.start_line
            {
                nest.push(self.newline());
            }
            nest.push(self.raw_text(dedented_raw(comment.text)));
            nest.push(self.newline());
            prev_end_line = Some(comment.end_line);
        }

        if !nest.is_empty() {
            list.push(self.newline());
            list.push(self.nest(self.trim_list(nest), self.indent));
        } else if !open_trailing.is_empty() {
            // The trailed `{` cannot be closed on its own line.
            list.push(self.newline());
        }
        list.push(self.token(token::TokenKind::CloseBrace));

        self.list(list)
    }

    pub fn build_turbofish(
        &self,
        turbofish: &Loc<ast::TurbofishInner>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        match &**turbofish {
            ast::TurbofishInner::Named(arguments) => self.list([
                self.token(token::TokenKind::PathSeparator),
                self.group(
                    "$<",
                    arguments,
                    token::TokenKind::Comma,
                    token::TokenKind::Gt.as_str(),
                    &turbofish.byte_range(),
                    comments,
                ),
            ]),
            ast::TurbofishInner::Positional(arguments) => self.list([
                self.token(token::TokenKind::PathSeparator),
                self.group(
                    token::TokenKind::Lt.as_str(),
                    arguments,
                    token::TokenKind::Comma,
                    token::TokenKind::Gt.as_str(),
                    &turbofish.byte_range(),
                    comments,
                ),
            ]),
        }
    }

    pub fn build_argument_list(
        &self,
        argument_list: &Loc<ast::ArgumentList>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        match &**argument_list {
            ast::ArgumentList::Positional(arguments) => self.group(
                token::TokenKind::OpenParen.as_str(),
                arguments,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                &argument_list.byte_range(),
                comments,
            ),
            ast::ArgumentList::Named(named_arguments) => self.group(
                "$(",
                named_arguments,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                &argument_list.byte_range(),
                comments,
            ),
        }
    }

    pub fn build_named_turbofish(
        &self,
        named_turbofish: &ast::NamedTurbofish,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        match named_turbofish {
            ast::NamedTurbofish::Full(name, value) => self.list([
                self.text(format!("{}: ", self.identifier_text(name))),
                self.build_type_expression(value, comments),
            ]),
            ast::NamedTurbofish::Short(name) => self.identifier(name),
        }
    }

    pub fn build_named_argument(
        &self,
        named_argument: &ast::NamedArgument,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        match named_argument {
            ast::NamedArgument::Full(name, current) => self.list([
                self.text(format!("{}: ", self.identifier_text(name))),
                self.build_expression(current, comments),
            ]),
            ast::NamedArgument::Short(name) => self.identifier(name),
        }
    }

    pub fn build_pattern(
        &self,
        pattern: &Loc<ast::Pattern>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let leading = self.inline_leading(pattern, comments);
        let doc = match &**pattern {
            ast::Pattern::Integer(_) | ast::Pattern::Bool(_) => {
                self.signed_literal(pattern.byte_range(), comments)
            }
            ast::Pattern::Bound(name, inner) => self.list([
                self.text(format!("{} @ ", self.identifier_text(name))),
                self.build_pattern(inner, comments),
            ]),
            ast::Pattern::Path { wire, path } => {
                let mut list = vec![];
                if wire.is_some() {
                    list.push(self.text("wire "));
                }
                list.push(self.build_path(path));
                self.list(list)
            }
            ast::Pattern::Tuple(tuple) => self.group(
                token::TokenKind::OpenParen.as_str(),
                tuple,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                &pattern.byte_range(),
                comments,
            ),
            ast::Pattern::Array(elements) => self.group(
                token::TokenKind::OpenBracket.as_str(),
                elements,
                token::TokenKind::Comma,
                token::TokenKind::CloseBracket.as_str(),
                &pattern.byte_range(),
                comments,
            ),
            ast::Pattern::Type(name, argument_pattern) => self.list([
                self.build_path(name),
                self.build_argument_pattern(argument_pattern, comments),
            ]),
        };
        self.with_inline_leading(leading, doc)
    }

    /// Claims the inline-leading comments of a construct that must not
    /// pull whole-line comments inward.
    fn inline_leading<'source>(
        &self,
        construct: &impl HasSourceRange,
        comments: &mut CommentMap<'source>,
    ) -> Vec<CommentToPrint<'source>> {
        comments.take_inline_leading(construct.byte_range().start)
    }

    /// Prefixes `doc` with claimed inline-leading comments.
    fn with_inline_leading(
        &self,
        leading: Vec<CommentToPrint>,
        doc: DocumentIdx,
    ) -> DocumentIdx {
        if leading.is_empty() {
            return doc;
        }
        let mut list = vec![];
        for comment in &leading {
            list.push(self.raw_text(comment.text));
            list.push(self.text(" "));
        }
        list.push(doc);
        self.list(list)
    }

    pub fn build_argument_pattern(
        &self,
        argument_pattern: &Loc<ast::ArgumentPattern>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        match &**argument_pattern {
            ast::ArgumentPattern::Named(arguments) => self.group(
                "$(",
                arguments,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                &argument_pattern.byte_range(),
                comments,
            ),
            ast::ArgumentPattern::Positional(tuple) => self.group(
                token::TokenKind::OpenParen.as_str(),
                tuple,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                &argument_pattern.byte_range(),
                comments,
            ),
        }
    }

    pub fn build_named_pattern_argument(
        &self,
        argument: &AstNamedPatternArgument,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        match &argument.1 {
            Some(pattern) => self.list([
                self.text(format!("{}: ", self.identifier_text(&argument.0))),
                self.build_pattern(pattern, comments),
            ]),
            None => self.identifier(&argument.0),
        }
    }

    pub fn build_type_expression(
        &self,
        type_expression: &Loc<ast::TypeExpression>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        match &**type_expression {
            ast::TypeExpression::TypeSpec(type_spec) => {
                self.build_type_spec(type_spec, comments)
            }
            ast::TypeExpression::Bool(_) | ast::TypeExpression::Integer(_) => {
                self.signed_literal(type_expression.byte_range(), comments)
            }
            // Const generics are always brace-delimited in type position.
            ast::TypeExpression::ConstGeneric(expression) => self.list([
                self.text("{"),
                self.build_expression(expression, comments),
                self.text("}"),
            ]),
            ast::TypeExpression::String(value) => self.string_literal(value),
        }
    }

    pub fn build_type_spec(
        &self,
        type_spec: &Loc<ast::TypeSpec>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let leading = self.inline_leading(type_spec, comments);
        let doc = match &**type_spec {
            ast::TypeSpec::Tuple(elements) => self.group(
                token::TokenKind::OpenParen.as_str(),
                elements,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                &type_spec.byte_range(),
                comments,
            ),
            ast::TypeSpec::Array { inner, size } => self.list([
                self.token(token::TokenKind::OpenBracket),
                self.build_type_expression(inner, comments),
                self.token(token::TokenKind::Semi),
                self.text(" "),
                self.build_type_expression(size, comments),
                self.token(token::TokenKind::CloseBracket),
            ]),
            ast::TypeSpec::Named(path, type_params) => {
                let mut list = vec![self.build_path(path)];
                if let Some(params) = type_params {
                    list.push(self.group(
                        token::TokenKind::Lt.as_str(),
                        &params.inner,
                        token::TokenKind::Comma,
                        token::TokenKind::Gt.as_str(),
                        &params.byte_range(),
                        comments,
                    ));
                }
                self.list(list)
            }
            ast::TypeSpec::Inverted(inner) => self.list([
                self.text("inv "),
                self.build_type_expression(inner, comments),
            ]),
            ast::TypeSpec::CopyView(inner) => self.list([
                self.text("&"),
                self.build_type_expression(inner, comments),
            ]),
            ast::TypeSpec::Impl(traits) => {
                let mut list = vec![self.text("impl ")];
                for (i, trait_spec) in traits.iter().enumerate() {
                    if i > 0 {
                        list.push(self.text(" + "));
                    }
                    list.push(self.build_trait_spec(trait_spec, comments));
                }
                self.list(list)
            }
            ast::TypeSpec::Wildcard => self.text("_"),
        };
        self.with_inline_leading(leading, doc)
    }

    pub fn build_type_param(
        &self,
        type_param: &Loc<ast::TypeParam>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        match &**type_param {
            ast::TypeParam::TypeName {
                name,
                traits,
                default,
            } => {
                let mut list = vec![self.identifier(name)];
                if !traits.is_empty() {
                    let mut flatten_list = vec![];
                    let mut nest_list = vec![];
                    for (i, trait_spec) in traits.iter().enumerate() {
                        if i > 0 {
                            flatten_list.push(self.text(format!(
                                " {} ",
                                token::TokenKind::Plus.as_str()
                            )));
                            nest_list.extend([
                                self.newline(),
                                self.text(format!(
                                    "{} ",
                                    token::TokenKind::Plus.as_str()
                                )),
                            ])
                        }
                        // Built once so comment claims land in both
                        // layouts.
                        let spec_doc =
                            self.build_trait_spec(trait_spec, comments);
                        flatten_list.push(spec_doc);
                        nest_list.push(spec_doc);
                    }
                    list.extend([
                        self.text(": "),
                        self.try_catch(
                            self.flatten(self.list(flatten_list)),
                            self.nest(self.list(nest_list), self.indent),
                        ),
                    ])
                }
                if let Some(default) = default {
                    list.extend([
                        self.text(" = "),
                        self.build_type_expression(default, comments),
                    ]);
                }
                self.list(list)
            }
            ast::TypeParam::TypeWithMeta {
                meta,
                name,
                default,
            } => {
                let mut list =
                    vec![self.text(format!(
                        "#{meta} {}",
                        self.identifier_text(name)
                    ))];
                if let Some(default) = default {
                    list.extend([
                        self.text(" = "),
                        self.build_type_expression(default, comments),
                    ]);
                }
                self.list(list)
            }
        }
    }

    pub fn build_trait_spec(
        &self,
        trait_spec: &Loc<ast::TraitSpec>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        // With `paren_syntax`, the parser appended the argument tuple and
        // the return type as the last two type params (an empty tuple
        // stands in for a missing `-> O`, so an explicit `-> ()` prints
        // without the arrow).
        if trait_spec.paren_syntax {
            let params = trait_spec
                .type_params
                .as_deref()
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            // Not producible by the parser; fail soft regardless.
            let [explicit @ .., args, output] = params else {
                return self
                    .unsupported(trait_spec, "this parenthesized trait bound");
            };
            let ast::TypeExpression::TypeSpec(args_spec) = &**args else {
                return self
                    .unsupported(trait_spec, "this parenthesized trait bound");
            };
            let ast::TypeSpec::Tuple(argument_types) = &***args_spec else {
                return self
                    .unsupported(trait_spec, "this parenthesized trait bound");
            };
            let mut list = vec![self.build_path(&trait_spec.path)];
            if !explicit.is_empty() {
                list.push(self.group(
                    token::TokenKind::Lt.as_str(),
                    explicit,
                    token::TokenKind::Comma,
                    token::TokenKind::Gt.as_str(),
                    &trait_spec.byte_range(),
                    comments,
                ));
            }
            list.push(self.group(
                token::TokenKind::OpenParen.as_str(),
                argument_types,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                &trait_spec.byte_range(),
                comments,
            ));
            let output_is_unit = matches!(
                &**output,
                ast::TypeExpression::TypeSpec(spec)
                    if matches!(&***spec, ast::TypeSpec::Tuple(elements)
                        if elements.is_empty())
            );
            if !output_is_unit {
                list.extend([
                    self.text(" -> "),
                    self.build_type_expression(output, comments),
                ]);
            }
            return self.list(list);
        }
        let mut list = vec![self.build_path(&trait_spec.path)];
        if let Some(type_params) = &trait_spec.type_params {
            list.push(self.group(
                token::TokenKind::Lt.as_str(),
                &type_params.inner,
                token::TokenKind::Comma,
                token::TokenKind::Gt.as_str(),
                &type_params.byte_range(),
                comments,
            ));
        }
        self.list(list)
    }

    pub fn build_attribute(
        &self,
        attribute: &Loc<ast::Attribute>,
    ) -> DocumentIdx {
        match &**attribute {
            ast::Attribute::Optimize { passes } => self.text(format!(
                "#[optimize({})]",
                passes
                    .iter()
                    .map(|pass| pass.inner.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            ast::Attribute::NoMangle { all } => self.text(format!(
                "#[no_mangle{}]",
                if *all { "(all)" } else { "" }
            )),
            ast::Attribute::Fsm { state } => match state {
                Some(state) => self
                    .text(format!("#[fsm({})]", self.identifier_text(state))),
                None => self.text("#[fsm]"),
            },
            ast::Attribute::Documentation { content } => {
                self.text(format!("///{content}"))
            }
            ast::Attribute::SurferTranslator(name) => self.list([
                self.text("#[surfer_translator("),
                self.string_literal(name),
                self.text(")]"),
            ]),
            ast::Attribute::SpadecParenSugar => {
                self.text("#[spadec_paren_sugar]")
            }
            ast::Attribute::Inline => self.text("#[inline]"),
            // `(note = "..")` parses to the same AST as `= ".."` and
            // prints as the latter; named arguments parse in any order
            // and print as `since, note`.
            ast::Attribute::Deprecated { since, note } => match (since, note) {
                (None, None) => self.text("#[deprecated]"),
                (None, Some(note)) => self.list([
                    self.text("#[deprecated = "),
                    self.string_literal(&note.inner),
                    self.text("]"),
                ]),
                (Some(since), None) => self.list([
                    self.text("#[deprecated(since = "),
                    self.string_literal(&since.inner),
                    self.text(")]"),
                ]),
                (Some(since), Some(note)) => self.list([
                    self.text("#[deprecated(since = "),
                    self.string_literal(&since.inner),
                    self.text(", note = "),
                    self.string_literal(&note.inner),
                    self.text(")]"),
                ]),
            },
            ast::Attribute::VerilogAttrs { entries } => {
                let mut list = vec![self.text("#[verilog_attrs(")];
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        list.push(self.text(", "));
                    }
                    list.push(self.identifier(key));
                    if let Some(value) = value {
                        list.push(self.text(" = "));
                        list.push(self.string_literal(&value.inner));
                    }
                }
                list.push(self.text(")]"));
                self.list(list)
            }
        }
    }

    pub fn build_attribute_list(
        &self,
        attribute_list: &ast::AttributeList,
        always_newline: bool,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let multiple = attribute_list.0.len() > 1;
        let mut list = vec![];
        for attribute in &attribute_list.0 {
            // A comment between attribute or doc lines stays there.
            let start = attribute.byte_range().start;
            let leading = comments.take_leading(start);
            self.emit_leading(&leading, self.line_of(start), &mut list);
            list.push(self.build_attribute(attribute));
            // A `///` doc renders as a line comment and must end its
            // line even in inline positions; the enclosing layout is
            // forced broken via [`Self::parameters_have_doc`].
            list.push(
                if multiple
                    || always_newline
                    || matches!(
                        **attribute,
                        ast::Attribute::Documentation { .. }
                    )
                {
                    self.newline()
                } else {
                    self.text(" ")
                },
            );
        }
        self.list(list)
    }

    pub fn build_parameter(
        &self,
        parameter: &AstParameter,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let mut list =
            vec![self.build_attribute_list(&parameter.0, false, comments)];
        if parameter.1.is_some() {
            list.push(self.text("wire "));
        }
        list.extend([
            self.text(format!("{}: ", self.identifier_text(&parameter.2))),
            self.build_type_spec(&parameter.3, comments),
        ]);
        self.list(list)
    }

    pub fn build_array_element(
        &self,
        element: &AstArrayElement,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let mut list = vec![];
        if let Some(label) = &element.0 {
            list.push(self.text(format!("'{} ", self.identifier_text(label))));
        }
        list.push(self.build_expression(&element.1, comments));
        self.list(list)
    }

    /// The rendered `pub` prefix (trailing space included), or [`None`] for
    /// implicit visibility.
    fn visibility_prefix(
        &self,
        visibility: &Loc<Visibility>,
    ) -> Option<DocumentIdx> {
        let keyword = match visibility.inner {
            Visibility::Implicit => return None,
            Visibility::Public => "pub ",
            Visibility::AtLib => "pub(lib) ",
            Visibility::AtSelf => "pub(self) ",
            Visibility::AtSuper => "pub(super) ",
            // Not producible by the parser today; fail soft regardless.
            Visibility::AtSuperSuper => {
                return Some(
                    self.unsupported(visibility, "this visibility level"),
                );
            }
        };
        Some(self.text(keyword))
    }

    /// Whether any parameter (`self` included) carries a `///` doc, which
    /// renders as a line comment and so rules out any flat layout.
    fn parameters_have_doc(parameter_list: &ast::ParameterList) -> bool {
        let has_doc = |attributes: &ast::AttributeList| {
            attributes.0.iter().any(|attribute| {
                matches!(**attribute, ast::Attribute::Documentation { .. })
            })
        };
        parameter_list
            .self_
            .as_ref()
            .is_some_and(|(attributes, _, _)| has_doc(attributes))
            || parameter_list
                .args
                .iter()
                .any(|(attributes, _, _, _)| has_doc(attributes))
    }

    /// Returns the (try, catch) pair of documents for `parameter_list`
    /// plus whether a comment forces the catch.
    pub fn build_parameter_list(
        &self,
        parameter_list: &Loc<ast::ParameterList>,
        comments: &mut CommentMap,
    ) -> (DocumentIdx, DocumentIdx, bool) {
        let continues = !parameter_list.args.is_empty();
        // `self` is not a group element, so it anchors its own comments
        // (claimed before the group sweeps its range); the attribute
        // list's span covers the keyword.
        let self_docs =
            parameter_list
                .self_
                .as_ref()
                .map(|(attributes, wire, amp)| {
                    let attributes_doc =
                        self.build_attribute_list(attributes, false, comments);
                    let range = attributed_span(attributes, attributes.span)
                        .byte_range();
                    let leading = comments.take_leading(range.start);
                    let line = self.line_of(range.start);
                    let trailing = comments
                        .take_trailing(range.end, self.line_of(range.end));
                    // Block comments precede the separator, line comments
                    // follow it.
                    let (line_comments, block_comments): (Vec<_>, Vec<_>) =
                        trailing
                            .into_iter()
                            .partition(|comment| comment.is_line);
                    let self_doc = |separator: &str| {
                        let mut list = vec![];
                        self.emit_leading(&leading, line, &mut list);
                        list.push(attributes_doc);
                        if wire.is_some() {
                            list.push(self.text("wire "));
                        }
                        if amp.is_some() {
                            list.push(self.text("&"));
                        }
                        list.push(self.text("self"));
                        self.emit_trailing(&block_comments, &mut list);
                        list.push(self.text(separator));
                        self.emit_trailing(&line_comments, &mut list);
                        self.list(list)
                    };
                    (self_doc(if continues { ", " } else { "" }), self_doc(","))
                });
        let (try_idx, catch_idx, force) = self.group_raw(
            &parameter_list.byte_range(),
            &parameter_list.args,
            token::TokenKind::Comma,
            comments,
        );
        let Some((flat_self, broken_self)) = self_docs else {
            return (try_idx, catch_idx, force);
        };
        // A lone `self` still closes on its own line.
        let catch_idx = if !continues && self.is_empty(catch_idx) {
            self.newline()
        } else {
            catch_idx
        };
        (
            self.list([flat_self, try_idx]),
            self.list([
                self.newline(),
                self.nest(broken_self, self.indent),
                catch_idx,
            ]),
            force,
        )
    }

    fn newline(&self) -> DocumentIdx {
        self.inner.borrow_mut().add(Document::Newline)
    }

    fn text(&self, text: impl Into<String>) -> DocumentIdx {
        let text = text.into();
        debug_assert!(
            !text.contains('\n'),
            "line breaks are Newline documents: {text:?}"
        );
        self.inner.borrow_mut().add(Document::Text(text))
    }

    fn raw_text(&self, raw_text: impl Into<String>) -> DocumentIdx {
        self.inner.borrow_mut().add(Document::Raw(raw_text.into()))
    }

    fn verbatim(&self, text: impl Into<String>) -> DocumentIdx {
        self.inner.borrow_mut().add(Document::Verbatim(text.into()))
    }

    /// A string literal, which may hold raw newlines. Spade strings have
    /// no escape sequences, so the parsed value is the exact source text.
    fn string_literal(&self, value: impl std::fmt::Display) -> DocumentIdx {
        self.verbatim(format!("\"{value}\""))
    }

    fn token(&self, text: token::TokenKind) -> DocumentIdx {
        self.text(text.as_str())
    }

    fn nest(&self, body: DocumentIdx, by: isize) -> DocumentIdx {
        self.inner.borrow_mut().add(Document::Nest(body, by))
    }

    fn flatten(&self, body: DocumentIdx) -> DocumentIdx {
        self.inner.borrow_mut().add(Document::Flatten(body))
    }

    fn try_catch(
        &self,
        try_body: DocumentIdx,
        catch_body: DocumentIdx,
    ) -> DocumentIdx {
        self.inner
            .borrow_mut()
            .add(Document::TryCatch(try_body, catch_body))
    }

    fn list(&self, list: impl IntoIterator<Item = DocumentIdx>) -> DocumentIdx {
        self.inner
            .borrow_mut()
            .add(Document::List(list.into_iter().collect()))
    }

    fn trim_list(
        &self,
        list: impl IntoIterator<Item = DocumentIdx>,
    ) -> DocumentIdx {
        let mut trimmed = list
            .into_iter()
            .skip_while(|idx| *idx == self.newline())
            .collect::<Vec<_>>();
        while let Some(last) = trimmed.last()
            && let Some(penultimate) = trimmed
                .len()
                .checked_sub(2)
                .and_then(|index| trimmed.get(index))
            && *last == self.newline()
            && *penultimate == self.newline()
        {
            trimmed.pop();
        }
        self.inner.borrow_mut().add(Document::List(trimmed))
    }

    /// Builds the elements of a delimiter-separated group whose source
    /// (delimiters included) spans `range`, claiming each element's
    /// leading and trailing comments. Returns the flat and broken bodies
    /// plus whether a comment forces the broken one.
    fn group_raw<'a, B: BuildAsDocument + HasSourceRange + 'a>(
        &self,
        range: &Range<usize>,
        contents: impl IntoIterator<Item = &'a B>,
        between: impl Into<Option<token::TokenKind>>,
        comments: &mut CommentMap,
    ) -> (DocumentIdx, DocumentIdx, bool) {
        let between = between.into();
        let elements: Vec<&B> = contents.into_iter().collect();
        let Some(first) = elements.first() else {
            return self.empty_group(range, comments);
        };
        let force = comments.forces_break(range);

        let open_trailing =
            comments.take_open_trailing(range.start, first.byte_range().start);

        let mut list = vec![];
        // Trailing line comments; unlike block ones they cannot precede
        // the separator, so they wait for it.
        let mut pending: Vec<CommentToPrint> = vec![];
        let mut prev_end_line = 0;
        let mut last_end_byte = range.start;
        for (i, element) in elements.iter().enumerate() {
            let element_range = element.byte_range();
            let leading = comments.take_leading(element_range.start);
            let line = self.line_of(element_range.start);
            let effective_start = leading
                .first()
                .map(|comment| comment.start_line)
                .unwrap_or(line);
            if i > 0 {
                if let Some(ref between) = between {
                    list.push(self.token(between.clone()));
                }
                let held = std::mem::take(&mut pending);
                self.emit_trailing(&held, &mut list);
                list.push(self.newline());
                if prev_end_line + 1 < effective_start {
                    list.push(self.newline());
                }
            }
            self.emit_leading(&leading, line, &mut list);
            list.push(element.build(self, comments));
            let end_line = self.line_of(element_range.end);
            let trailing = comments.take_trailing(element_range.end, end_line);
            prev_end_line = trailing
                .last()
                .map(|comment| comment.end_line)
                .unwrap_or(end_line);
            for comment in trailing {
                if comment.is_line {
                    pending.push(comment);
                } else {
                    self.emit_trailing(&[comment], &mut list);
                }
            }
            last_end_byte = element_range.end;
        }
        let doc_contents = self.list(list);

        let mut inner = vec![doc_contents];
        if matches!(between, Some(token::TokenKind::Comma)) {
            // always trailing comma when nesting a comma group, could
            // overestimate
            inner.push(self.token(token::TokenKind::Comma));
        }
        self.emit_trailing(&pending, &mut inner);
        // Scope-trailing comments between the last element and the
        // closing delimiter.
        let rest = comments.take_between(last_end_byte..range.end);
        for comment in &rest {
            inner.push(self.newline());
            if prev_end_line + 1 < comment.start_line {
                inner.push(self.newline());
            }
            inner.push(self.raw_text(dedented_raw(comment.text)));
            prev_end_line = comment.end_line;
        }

        let mut nest_list = vec![];
        self.emit_trailing(&open_trailing, &mut nest_list);
        nest_list.extend([
            self.newline(),
            self.nest(self.list(inner), self.indent),
            self.newline(),
        ]);
        // try to flatten, otherwise nest
        (self.flatten(doc_contents), self.list(nest_list), force)
    }

    /// [`Self::group_raw`] for a group without elements: the comments
    /// between its delimiters print side by side in the flat body and one
    /// per line in the broken one. Both bodies are empty when there are
    /// none, so the delimiters close on themselves.
    fn empty_group(
        &self,
        range: &Range<usize>,
        comments: &mut CommentMap,
    ) -> (DocumentIdx, DocumentIdx, bool) {
        let force = comments.forces_break(range);
        let inside = comments.take_within(range);
        if inside.is_empty() {
            return (self.list([]), self.list([]), false);
        }

        let mut flat = vec![];
        for (i, comment) in inside.iter().enumerate() {
            if i > 0 {
                flat.push(self.text(" "));
            }
            flat.push(self.raw_text(dedented_raw(comment.text)));
        }
        let mut broken = vec![];
        self.emit_scope_trailing(&inside, None, &mut broken);
        let broken = self.list([
            self.newline(),
            self.nest(self.list(broken), self.indent),
            self.newline(),
        ]);
        (self.flatten(self.list(flat)), broken, force)
    }

    /// Whether `idx` is the empty list, which prints nothing.
    fn is_empty(&self, idx: DocumentIdx) -> bool {
        idx == self.list([])
    }

    fn group<'a, B: BuildAsDocument + HasSourceRange + 'a>(
        &self,
        open: impl Into<String>,
        contents: impl IntoIterator<Item = &'a B>,
        between: impl Into<Option<token::TokenKind>>,
        close: impl Into<String>,
        range: &Range<usize>,
        comments: &mut CommentMap,
    ) -> DocumentIdx {
        let open = open.into();
        let close = close.into();

        let (try_body_idx, catch_body_idx, force) =
            self.group_raw(range, contents, between, comments);
        let catch_doc = self.list([
            self.text(open.clone()),
            catch_body_idx,
            self.text(close.clone()),
        ]);
        if force {
            return catch_doc;
        }
        let try_doc =
            self.list([self.text(open), try_body_idx, self.text(close)]);
        self.try_catch(try_doc, catch_doc)
    }
}
