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

use std::cell::RefCell;

use spade_ast as ast;
use spade_ast::token;
use spade_codespan_reporting::files::{Files, SimpleFile};
use spade_common::{
    location_info::{FullSpan, Loc, WithLocation},
    name::{Identifier, Path, Visibility},
};
use spade_diagnostics::{Diagnostic, codespan::Span};

use crate::{
    comment_insertion::CommentInserter,
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
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx;
}

impl BuildAsDocument for Loc<DocumentIdx> {
    fn build(
        &self,
        _builder: &DocumentBuilder,
        _comment_inserter: &mut CommentInserter,
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
                comment_inserter: &mut CommentInserter,
            ) -> $crate::document::DocumentIdx {
                builder.$name(self, comment_inserter)
            }
        }

        impl BuildAsDocument for Loc<$T> {
            fn build(
                &self,
                builder: &DocumentBuilder,
                comment_inserter: &mut CommentInserter,
            ) -> $crate::document::DocumentIdx {
                builder.$name(self, comment_inserter)
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

pub trait HasLineNumber {
    fn line_index(&self, builder: &DocumentBuilder) -> usize;
    fn end_line_index(&self, builder: &DocumentBuilder) -> usize;
}

impl HasLineNumber for Span {
    fn line_index(&self, builder: &DocumentBuilder) -> usize {
        builder
            .file
            .borrow()
            .unwrap()
            .line_index((), self.start().to_usize())
            .expect("span was somehow not from the file it came from")
    }

    fn end_line_index(&self, builder: &DocumentBuilder) -> usize {
        builder
            .file
            .borrow()
            .unwrap()
            .line_index((), self.end().to_usize())
            .expect("span was somehow not from the file it came from")
    }
}

impl<T> HasLineNumber for Loc<T> {
    fn line_index(&self, builder: &DocumentBuilder) -> usize {
        self.span.line_index(builder)
    }

    fn end_line_index(&self, builder: &DocumentBuilder) -> usize {
        self.span.end_line_index(builder)
    }
}

impl HasLineNumber for ast::EnumVariant {
    fn line_index(&self, builder: &DocumentBuilder) -> usize {
        self.name.line_index(builder)
    }

    fn end_line_index(&self, builder: &DocumentBuilder) -> usize {
        self.name.end_line_index(builder)
    }
}

impl HasLineNumber for ast::NamedArgument {
    fn line_index(&self, builder: &DocumentBuilder) -> usize {
        match self {
            ast::NamedArgument::Full(name, _)
            | ast::NamedArgument::Short(name) => name.line_index(builder),
        }
    }

    fn end_line_index(&self, builder: &DocumentBuilder) -> usize {
        match self {
            ast::NamedArgument::Full(name, _)
            | ast::NamedArgument::Short(name) => name.end_line_index(builder),
        }
    }
}

impl HasLineNumber for AstParameter {
    fn line_index(&self, builder: &DocumentBuilder) -> usize {
        self.0
            .0
            .first()
            .map(|first| first.span)
            .unwrap_or(self.2.span)
            .line_index(builder)
    }

    fn end_line_index(&self, builder: &DocumentBuilder) -> usize {
        self.0
            .0
            .first()
            .map(|first| first.span)
            .unwrap_or(self.2.span)
            .end_line_index(builder)
    }
}

impl HasLineNumber for AstNamedPatternArgument {
    fn line_index(&self, builder: &DocumentBuilder) -> usize {
        self.0.line_index(builder)
    }

    fn end_line_index(&self, builder: &DocumentBuilder) -> usize {
        self.1
            .as_ref()
            .map(|pattern| pattern.span)
            .unwrap_or(self.0.span)
            .end_line_index(builder)
    }
}

impl HasLineNumber for AstArrayElement {
    fn line_index(&self, builder: &DocumentBuilder) -> usize {
        self.0
            .as_ref()
            .map(|label| label.span)
            .unwrap_or(self.1.span)
            .line_index(builder)
    }

    fn end_line_index(&self, builder: &DocumentBuilder) -> usize {
        self.1.end_line_index(builder)
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

fn where_clause_target(clause: &ast::WhereClause) -> &Loc<Path> {
    match clause {
        ast::WhereClause::GenericInt { target, .. }
        | ast::WhereClause::TraitBounds { target, .. } => target,
    }
}

fn span_of_item(item: &ast::Item) -> Span {
    match item {
        spade_ast::Item::Unit(unit) => unit.span,
        spade_ast::Item::MacroDef(macro_def) => macro_def.span,
        spade_ast::Item::TraitDef(trait_definition) => trait_definition.span,
        spade_ast::Item::Type(ty) => ty.span,
        spade_ast::Item::ExternalMod(external_module) => external_module.span,
        spade_ast::Item::Module(module) => module.span,
        spade_ast::Item::Use(_, use_) => use_.span,
        spade_ast::Item::ImplBlock(impl_block) => impl_block.span,
    }
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

    /// Pull comments from `comment_inserter` **exclusively** till
    /// `end_line_index`.
    ///
    /// `update_last_line_index`, if provided, will be set to the last line
    /// index of the comment if one exists. (If one does not exist, it will not
    /// be set.)
    fn pull_comments(
        &self,
        comment_inserter: &mut CommentInserter,
        start_line_index: usize,
        end_line_index: usize,
        trailing_newline: bool,
        update_last_line_index: Option<&mut usize>,
    ) -> Vec<DocumentIdx> {
        let comments = comment_inserter.get_comments_temp(
            self.file.borrow().unwrap(),
            start_line_index,
            end_line_index,
        );
        let file = self.file.borrow().unwrap();

        let mut result = vec![];
        let mut last_line_index = start_line_index;
        for comment in &comments {
            let comment_line_index = comment.start_line(file);
            if last_line_index + 1 < comment_line_index {
                result.push(self.newline());
            }
            result.extend([self.raw_text(comment.source), self.newline()]);
            last_line_index = comment.end_line(file);
        }
        if !result.is_empty() {
            if !trailing_newline {
                result.pop();
            }
            if let Some(update_last_line_index) = update_last_line_index {
                *update_last_line_index =
                    comments.last().unwrap().end_line(file);
            }
        }
        result
    }

    pub fn build_root(
        self,
        root: &Loc<ast::ModuleBody>,
        file: &'code SimpleFile<String, String>,
        comment_inserter: &mut CommentInserter,
    ) -> (InternedDocumentStore, DocumentIdx, Vec<Diagnostic>) {
        self.file.replace(Some(file));

        // `//!` docs are tokens, not comments, so the comment inserter
        // never sees them. They carry no spans; anchor at the file start.
        if !root.documentation.is_empty() {
            self.record_unsupported(
                (Span::new(0, 0), root.file_id),
                "module documentation (`//!` comments)",
            );
        }

        let mut list = vec![];

        let mut last_line_index = 0;
        for (i, item) in root.members.iter().enumerate() {
            let item_span = span_of_item(item);
            let item_line_index = item_span.line_index(&self);

            list.extend(self.pull_comments(
                comment_inserter,
                last_line_index,
                item_line_index,
                true,
                Some(&mut last_line_index),
            ));

            if i > 0 {
                if last_line_index + 1 < item_line_index {
                    list.push(self.newline());
                }
                list.push(self.newline());
            }
            list.push(self.build_item(item, comment_inserter));
            last_line_index = item_span.end_line_index(&self);
        }

        list.extend(self.pull_comments(
            comment_inserter,
            last_line_index,
            usize::MAX,
            true,
            None,
        ));

        let idx = self.trim_list(list);
        (self.inner.take(), idx, self.diagnostics.take())
    }

    pub fn build_item(
        &self,
        item: &ast::Item,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match item {
            ast::Item::Unit(unit) => self.build_unit(unit, comment_inserter),
            ast::Item::MacroDef(macro_def) => {
                self.unsupported(macro_def, "macro definitions")
            }
            ast::Item::TraitDef(trait_definition) => {
                self.unsupported(trait_definition, "trait definitions")
            }
            ast::Item::Type(type_declaration) => {
                self.build_type_declaration(type_declaration, comment_inserter)
            }
            ast::Item::ExternalMod(external_module) => {
                self.unsupported(external_module, "external modules")
            }
            ast::Item::Module(module) => {
                self.build_module(module, comment_inserter)
            }
            ast::Item::Use(attributes, use_statements) => {
                self.build_use(attributes, use_statements)
            }
            ast::Item::ImplBlock(impl_block) => {
                self.build_impl_block(impl_block, comment_inserter)
            }
        }
    }

    pub fn build_unit(
        &self,
        unit: &Loc<ast::Unit>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        let mut list = vec![];

        list.push(self.build_attribute_list(&unit.head.attributes, true));

        if let Some(visibility) = self.visibility_prefix(&unit.head.visibility)
        {
            list.push(visibility);
        }
        if unit.head.unsafe_token.is_some() {
            list.push(self.text("unsafe "));
        }
        if unit.head.extern_token.is_some() {
            list.push(self.text("extern "));
        }

        list.push(match &*unit.head.unit_kind {
            ast::UnitKind::Function => self.text("fn"),
            ast::UnitKind::Entity => self.text("entity"),
            ast::UnitKind::Pipeline(depth) => self.list([
                self.text("pipeline("),
                self.build_type_expression(depth, comment_inserter),
                self.text(")"),
            ]),
        });

        list.push(self.text(format!(" {}", unit.head.name)));

        if let Some(type_params) = &unit.head.type_params {
            list.push(self.group(
                token::TokenKind::Lt.as_str(),
                &type_params.inner,
                token::TokenKind::Comma,
                token::TokenKind::Gt.as_str(),
                comment_inserter,
            ));
        }

        let parameter_list_doc =
            self.build_parameter_list(&unit.head.inputs, comment_inserter);
        let parameter_open = self.token(token::TokenKind::OpenParen);
        let parameter_close = self.token(token::TokenKind::CloseParen);

        let output_type_doc =
            if let Some((_, output_type)) = &unit.head.output_type {
                self.list([
                    self.text(" -> "),
                    self.build_type_spec(output_type, comment_inserter),
                ])
            } else {
                self.list([])
            };

        list.push(self.try_catch(
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
                self.list([
                    parameter_open,
                    parameter_list_doc.1,
                    parameter_close,
                    output_type_doc,
                ]),
            ),
        ));

        if let Some(clause) = unit.head.where_clauses.first() {
            self.record_unsupported(
                where_clause_target(clause),
                "`where` clauses",
            );
        }

        list.push(match &unit.body {
            Some(body) => self.list([
                self.text(" "),
                self.build_expression(body, comment_inserter),
            ]),
            None => self.text(";"),
        });

        self.list(list)
    }

    pub fn build_type_declaration(
        &self,
        type_declaration: &Loc<ast::TypeDeclaration>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        let visibility = self.visibility_prefix(&type_declaration.visibility);
        match &type_declaration.kind {
            ast::TypeDeclKind::Enum(enum_decl) => {
                let mut list = vec![
                    self.build_attribute_list(&enum_decl.attributes, true),
                ];
                list.extend(visibility);
                list.push(self.text("enum "));
                list.push(self.text(enum_decl.name.to_string()));
                if let Some(generic_args) = &type_declaration.generic_args {
                    list.push(self.group(
                        token::TokenKind::Lt.as_str(),
                        &generic_args.inner,
                        token::TokenKind::Comma,
                        token::TokenKind::Gt.as_str(),
                        comment_inserter,
                    ));
                }
                let options_doc = self.group_raw(
                    &enum_decl.variants,
                    token::TokenKind::Comma,
                    comment_inserter,
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
                let mut list = vec![
                    self.build_attribute_list(&struct_decl.attributes, true),
                ];
                list.extend(visibility);
                list.push(self.text("struct "));
                list.push(self.text(struct_decl.name.to_string()));
                if let Some(generic_args) = &type_declaration.generic_args {
                    list.push(self.group(
                        token::TokenKind::Lt.as_str(),
                        &generic_args.inner,
                        token::TokenKind::Comma,
                        token::TokenKind::Gt.as_str(),
                        comment_inserter,
                    ));
                }
                let parameter_list_doc = self.build_parameter_list(
                    &struct_decl.members,
                    comment_inserter,
                );
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
                let mut list =
                    vec![self.build_attribute_list(&alias.attributes, true)];
                list.extend(visibility);
                list.push(self.text("type "));
                list.push(self.text(alias.name.to_string()));
                if let Some(generic_args) = &type_declaration.generic_args {
                    list.push(self.group(
                        token::TokenKind::Lt.as_str(),
                        &generic_args.inner,
                        token::TokenKind::Comma,
                        token::TokenKind::Gt.as_str(),
                        comment_inserter,
                    ));
                }
                list.extend([
                    self.text(" = "),
                    self.build_type_spec(&alias.type_spec, comment_inserter),
                    self.token(token::TokenKind::Semi),
                ]);
                self.list(list)
            }
        }
    }

    pub fn build_enum_variant(
        &self,
        variant: &ast::EnumVariant,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        // Variants render inside flattenable groups, where attribute and
        // doc lines cannot be emitted safely yet.
        if let Some(attribute) = variant.attributes.0.first() {
            self.record_unsupported(
                attribute,
                "attributes or documentation on enum variants",
            );
        }
        let mut list = vec![self.text(variant.name.to_string())];
        if let Some(parameter_list) = &variant.args {
            let parameter_list_doc =
                self.build_parameter_list(parameter_list, comment_inserter);
            list.extend([
                self.text(" {"),
                self.try_catch(
                    self.list([
                        self.text(" "),
                        parameter_list_doc.0,
                        self.text(" "),
                    ]),
                    parameter_list_doc.1,
                ),
                self.text("}"),
            ]);
        }
        self.list(list)
    }

    pub fn build_module(
        &self,
        item: &Loc<ast::Module>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        let mut list = vec![self.build_attribute_list(&item.attributes, true)];
        list.extend(self.visibility_prefix(&item.visibility));
        list.extend([
            self.text(format!("mod {} {{", item.name)),
            self.newline(),
            self.nest(
                self.build_module_body(&item.body, comment_inserter),
                self.indent,
            ),
            self.newline(),
            self.text("}"),
        ]);
        self.list(list)
    }

    pub fn build_module_body(
        &self,
        body: &Loc<ast::ModuleBody>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        // The doc strings carry no spans; anchor at the body start.
        if !body.documentation.is_empty() {
            self.record_unsupported(
                (
                    Span::new(body.span.start(), body.span.start()),
                    body.file_id,
                ),
                "module documentation (`//!` comments)",
            );
        }

        let mut list = vec![];
        let mut last_line_index = body.line_index(self);

        for (i, item) in body.members.iter().enumerate() {
            let item_span = span_of_item(item);
            let item_line_index = item_span.line_index(self);

            list.extend(self.pull_comments(
                comment_inserter,
                last_line_index,
                item_line_index,
                true,
                Some(&mut last_line_index),
            ));

            if i > 0 {
                if last_line_index + 1 < item_line_index {
                    list.push(self.newline());
                }
                list.push(self.newline());
            }
            list.push(self.build_item(item, comment_inserter));
            last_line_index = item_span.end_line_index(self);
        }

        list.extend(self.pull_comments(
            comment_inserter,
            last_line_index,
            body.span.end_line_index(self),
            true,
            None,
        ));

        self.list(list)
    }

    pub fn build_use(
        &self,
        attributes: &ast::AttributeList,
        use_statements: &Loc<Vec<ast::UseStatement>>,
    ) -> DocumentIdx {
        // `use a::{b, c}` expands to several statements whose brace tree
        // cannot be reconstructed from the AST alone yet.
        let [use_statement] = use_statements.inner.as_slice() else {
            return self
                .unsupported(use_statements, "`use` statements with braces");
        };
        let ast::UseStatement {
            visibility,
            path,
            alias,
        } = use_statement;

        let mut line = vec![self.build_attribute_list(attributes, true)];
        line.extend(self.visibility_prefix(visibility));
        line.extend([self.text("use "), self.build_path(path)]);

        if let Some(alias) = alias {
            line.push(self.text(format!(" as {alias}")));
        }

        line.push(self.text(";"));
        self.list(line)
    }

    pub fn build_impl_block(
        &self,
        impl_block: &Loc<ast::ImplBlock>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        let mut list = vec![self.text("impl")];
        if let Some(type_params) = &impl_block.type_params {
            list.push(self.group(
                token::TokenKind::Lt.as_str(),
                &type_params.inner,
                token::TokenKind::Comma,
                token::TokenKind::Gt.as_str(),
                comment_inserter,
            ));
        }
        list.push(self.text(" "));
        if let Some(impl_trait) = &impl_block.r#trait {
            list.extend([
                self.build_trait_spec(impl_trait, comment_inserter),
                self.text(" for "),
            ]);
        }
        list.push(self.build_type_spec(&impl_block.target, comment_inserter));

        if let Some(clause) = impl_block.where_clauses.first() {
            self.record_unsupported(
                where_clause_target(clause),
                "`where` clauses",
            );
        }
        if let Some(assoc_type) = impl_block.assoc_types.first() {
            self.record_unsupported(
                assoc_type,
                "associated types in `impl` blocks",
            );
        }

        list.push(self.text(" {"));
        if !impl_block.units.is_empty() {
            list.push(self.newline());
            let mut unit_list = vec![];
            for (i, unit) in impl_block.units.iter().enumerate() {
                if i > 0 {
                    unit_list.push(self.newline());
                }
                unit_list.push(self.build_unit(unit, comment_inserter))
            }
            list.push(self.nest(self.list(unit_list), self.indent));
            list.push(self.newline());
        }
        list.push(self.text("}"));

        self.list(list)
    }

    pub fn build_path(&self, path: &Loc<Path>) -> DocumentIdx {
        self.text(
            path.inner
                .0
                .iter()
                .map(|component| component.to_string())
                .collect::<Vec<_>>()
                .join("::"),
        )
    }

    pub fn build_statement(
        &self,
        statement: &Loc<ast::Statement>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        let (mut list, wants_semi) = match &**statement {
            ast::Statement::Label(name) => {
                (vec![self.text(format!("'{name}"))], false)
            }
            ast::Statement::Declaration(names) => (
                vec![self.text(format!(
                    "decl {}",
                    names
                        .iter()
                        .map(|name| name.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))],
                true,
            ),
            ast::Statement::Binding(binding) => {
                if let Some(attribute) = binding.attrs.0.first() {
                    self.record_unsupported(
                        attribute,
                        "attributes or documentation on `let` bindings",
                    );
                }
                let mut list = vec![
                    self.text("let "),
                    self.build_pattern(&binding.pattern, comment_inserter),
                ];

                if let Some(ty) = &binding.ty {
                    list.extend([
                        self.text(": "),
                        self.build_type_spec(ty, comment_inserter),
                    ]);
                }

                list.push(self.text(" = "));
                list.push(
                    self.build_expression(&binding.value, comment_inserter),
                );

                (list, true)
            }
            ast::Statement::PipelineRegMarker(count, condition) => {
                let mut list = vec![self.text("reg")];

                if let Some(condition) = condition {
                    list.extend([
                        self.token(token::TokenKind::OpenBracket),
                        self.build_expression(condition, comment_inserter),
                        self.token(token::TokenKind::CloseBracket),
                    ]);
                }

                if let Some(count) = count {
                    list.extend([
                        self.text(" * "),
                        self.build_type_expression(count, comment_inserter),
                    ]);
                }

                (list, true)
            }
            ast::Statement::Register(register) => {
                let mut list = vec![
                    self.build_attribute_list(&register.attributes, true),
                    self.text("reg("),
                    self.build_expression(&register.clock, comment_inserter),
                    self.text(") "),
                    self.build_pattern(&register.pattern, comment_inserter),
                ];

                if let Some(value_type) = &register.value_type {
                    list.extend([
                        self.text(": "),
                        self.build_type_spec(value_type, comment_inserter),
                    ]);
                }

                list.push(self.text(" "));

                if let Some(reset) = &register.reset {
                    list.extend([
                        self.text("reset("),
                        self.build_expression(&reset.0, comment_inserter),
                        self.text(": "),
                        self.build_expression(&reset.1, comment_inserter),
                        self.text(") "),
                    ]);
                }

                if let Some(initial) = &register.initial {
                    list.extend([
                        self.text("initial("),
                        self.build_expression(&initial, comment_inserter),
                        self.text(") "),
                    ]);
                }

                list.extend([
                    self.text("= "),
                    self.build_expression(&register.value, comment_inserter),
                ]);

                (list, true)
            }
            ast::Statement::Set { target, value } => (
                vec![
                    self.text("set "),
                    self.build_expression(target, comment_inserter),
                    self.text(" = "),
                    self.build_expression(value, comment_inserter),
                ],
                true,
            ),
            ast::Statement::Assert(expression) => (
                vec![
                    self.text("assert "),
                    self.build_expression(expression, comment_inserter),
                ],
                true,
            ),
            ast::Statement::Expression(expression, attributes) => (
                vec![
                    self.build_attribute_list(attributes, true),
                    self.build_expression(expression, comment_inserter),
                ],
                true,
            ),
            // Enum and struct take no semicolon; an alias owns its own.
            ast::Statement::Type(type_declaration) => (
                vec![self.build_type_declaration(
                    type_declaration,
                    comment_inserter,
                )],
                false,
            ),
        };
        if wants_semi {
            list.push(self.token(token::TokenKind::Semi));
        }

        let end_of_statement_comments = self.pull_comments(
            comment_inserter,
            statement.line_index(self),
            statement.end_line_index(self) + 1,
            false,
            None,
        );
        if !end_of_statement_comments.is_empty() {
            list.push(self.text(" "));
            list.extend(end_of_statement_comments);
        }

        self.list(list)
    }

    pub fn build_expression(
        &self,
        expression: &Loc<ast::Expression>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match &**expression {
            ast::Expression::Identifier(path) => self.build_path(path),
            ast::Expression::IntLiteral(int_literal) => {
                self.text(int_literal.to_string())
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
            ast::Expression::ArrayLiteral(elements) => self.group(
                token::TokenKind::OpenBracket.as_str(),
                elements,
                token::TokenKind::Comma,
                token::TokenKind::CloseBracket.as_str(),
                comment_inserter,
            ),
            ast::Expression::ArrayShorthandLiteral(element, amount) => self
                .list([
                    self.token(token::TokenKind::OpenBracket),
                    self.build_expression(element, comment_inserter),
                    self.token(token::TokenKind::Semi),
                    self.text(" "),
                    self.build_expression(amount, comment_inserter),
                    self.token(token::TokenKind::CloseBracket),
                ]),
            ast::Expression::Index(target, index) => self.list([
                self.build_expression(target, comment_inserter),
                self.token(token::TokenKind::OpenBracket),
                self.build_expression(index, comment_inserter),
                self.token(token::TokenKind::CloseBracket),
            ]),
            ast::Expression::RangeIndex { target, start, end } => {
                let mut list = vec![
                    self.build_expression(target, comment_inserter),
                    self.token(token::TokenKind::OpenBracket),
                ];
                if let Some(start) = start {
                    list.push(self.build_expression(start, comment_inserter));
                }
                list.push(self.token(token::TokenKind::DotDot));
                if let Some(end) = end {
                    list.push(self.build_expression(end, comment_inserter));
                }
                list.push(self.token(token::TokenKind::CloseBracket));
                self.list(list)
            }
            ast::Expression::TupleLiteral(items) => self.group(
                token::TokenKind::OpenParen.as_str(),
                items,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                comment_inserter,
            ),
            // The deprecated `x#0` syntax normalizes to `x.0`.
            ast::Expression::TupleIndex { target, index, .. } => self.list([
                self.build_expression(target, comment_inserter),
                self.text(format!(".{}", **index)),
            ]),
            ast::Expression::FieldAccess(parent, field) => self.list([
                self.build_expression(parent, comment_inserter),
                self.text(format!(".{field}")),
            ]),
            ast::Expression::TypeCast(target, ty) => self.list([
                self.build_expression(target, comment_inserter),
                self.text(" as "),
                self.build_type_expression(ty, comment_inserter),
            ]),
            ast::Expression::LabelAccess { label, field } => self.list([
                self.text("@"),
                self.build_path(label),
                self.text(format!(".{field}")),
            ]),
            ast::Expression::MacroCall { .. } => {
                self.unsupported(expression, "macro invocations")
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
                        self.build_type_expression(latency, comment_inserter),
                        self.text(") "),
                    ],
                };

                list.push(self.build_path(callee));
                if let Some(turbofish) = turbofish {
                    list.push(
                        self.build_turbofish(turbofish, comment_inserter),
                    );
                }
                list.push(self.build_argument_list(args, comment_inserter));

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
                    self.build_expression(target, comment_inserter),
                    self.token(token::TokenKind::Dot),
                ];
                list.extend(match kind {
                    ast::CallKind::Function => vec![],
                    ast::CallKind::Entity(_) => vec![self.text("inst ")],
                    ast::CallKind::Pipeline(_, latency) => vec![
                        self.text("inst("),
                        self.build_type_expression(latency, comment_inserter),
                        self.text(") "),
                    ],
                });

                list.push(self.text(name.to_string()));

                if let Some(turbofish) = turbofish {
                    list.push(self.build_turbofish(turbofish, comment_inserter))
                }

                list.push(self.build_argument_list(args, comment_inserter));

                self.list(list)
            }
            ast::Expression::If {
                cond,
                on_true,
                on_false,
            } => self.list([
                self.text("if "),
                self.build_expression(cond, comment_inserter),
                self.text(" "),
                self.build_expression(on_true, comment_inserter),
                self.text(" else "),
                self.build_expression(on_false, comment_inserter),
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
                            self.build_pattern(pattern, comment_inserter),
                            self.text(" = "),
                            self.build_expression(against, comment_inserter),
                            self.text(" "),
                            self.build_expression(on_true, comment_inserter),
                            self.text(" else "),
                            self.build_expression(on_false, comment_inserter),
                        ]);
                    }
                    // Unreachable from the parser.
                    return self
                        .unsupported(expression, "this `if let` expression");
                }
                let mut list = vec![
                    self.text("match "),
                    self.build_expression(against, comment_inserter),
                ];
                if !arms.is_empty() {
                    let mut arm_list = vec![];
                    for arm in &arms.inner {
                        let mut pattern_list =
                            vec![self.build_pattern(&arm.0, comment_inserter)];
                        if let Some(guard) = &arm.1 {
                            pattern_list.extend([
                                self.text(" if "),
                                self.build_expression(guard, comment_inserter),
                            ]);
                        }
                        let pattern = self.list(pattern_list);
                        let case = self.list([
                            self.text(format!(
                                " {} ",
                                token::TokenKind::FatArrow.as_str()
                            )),
                            self.build_expression(&arm.2, comment_inserter),
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
                            .at_loc(&arm.0),
                        );
                    }

                    let arms_doc = self.group_raw(
                        &arm_list,
                        token::TokenKind::Comma,
                        comment_inserter,
                    );
                    list.extend([
                        self.text(" {"),
                        self.try_catch(
                            self.list([
                                self.text(" "),
                                arms_doc.0,
                                self.text(" "),
                            ]),
                            arms_doc.1,
                        ),
                        self.text("}"),
                    ]);
                } else {
                    list.push(self.text(" {}"));
                }
                self.list(list)
            }
            // TODO: proper parenthesization in both of these
            ast::Expression::UnaryOperator(unary_operator, inner) => {
                self.list([
                    self.text(unary_operator_str(unary_operator)),
                    self.build_expression(inner, comment_inserter),
                ])
            }
            ast::Expression::BinaryOperator(left, op, right) => self.list([
                self.build_expression(left, comment_inserter),
                self.text(format!(" {} ", binary_operator_str(op))),
                self.build_expression(right, comment_inserter),
            ]),
            ast::Expression::Block(block) => self.build_block(
                block,
                expression.line_index(self),
                expression.end_line_index(self),
                comment_inserter,
            ),
            ast::Expression::PipelineReference { stage, name, .. } => {
                let stage_doc = match stage {
                    ast::PipelineStageReference::Absolute(identifier) => {
                        self.text(identifier.to_string())
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
                                                inner,
                                                comment_inserter,
                                            ),
                                        ])
                                    }
                                    _ => self.list([
                                        self.text("+"),
                                        self.build_expression(
                                            offset,
                                            comment_inserter,
                                        ),
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
                    self.text(format!(").{name}")),
                ])
            }
            ast::Expression::TypeLevelIf {
                cond,
                on_true,
                on_false,
            } => self.list([
                self.text("gen "),
                self.build_gen_if(cond, on_true, on_false, comment_inserter),
            ]),
            ast::Expression::StageValid => self.text("stage.valid"),
            ast::Expression::StageReady => self.text("stage.ready"),
            // Spade strings have no escape sequences, so the parsed
            // value is the exact source text.
            ast::Expression::StrLiteral(value) => {
                self.text(format!("\"{}\"", **value))
            }
            ast::Expression::Parenthesized(inner) => self.list([
                self.token(token::TokenKind::OpenParen),
                self.build_expression(inner, comment_inserter),
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
                        self.build_type_expression(depth, comment_inserter),
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
                        comment_inserter,
                    ));
                }
                list.push(self.text(" "));
                // A bare-expression body parses as a statement-less
                // block; print it back bare.
                match (&body.statements[..], &body.result) {
                    ([], Some(result)) => list
                        .push(self.build_expression(result, comment_inserter)),
                    _ => list.push(self.build_block(
                        body,
                        body.line_index(self),
                        body.end_line_index(self),
                        comment_inserter,
                    )),
                }
                self.list(list)
            }
            ast::Expression::Unsafe(block) => self.list([
                self.text("unsafe "),
                self.build_block(
                    block,
                    block.line_index(self),
                    block.end_line_index(self),
                    comment_inserter,
                ),
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
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        let mut list = vec![
            self.text("if "),
            self.build_expression(cond, comment_inserter),
            self.text(" "),
            self.build_expression(on_true, comment_inserter),
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
                } => {
                    self.build_gen_if(cond, on_true, on_false, comment_inserter)
                }
                _ => self.build_expression(on_false, comment_inserter),
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
        start_line_index: usize,
        end_line_index: usize,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        let mut list = vec![self.token(token::TokenKind::OpenBrace)];
        if block.statements.len() + block.result.as_ref().map_or(0, |_| 1) > 0 {
            list.push(self.newline());

            let mut nest = vec![];

            let mut last_line_index = start_line_index;
            for (i, statement) in block.statements.iter().enumerate() {
                let item_line_index = statement.line_index(self);

                nest.extend(self.pull_comments(
                    comment_inserter,
                    last_line_index,
                    item_line_index,
                    true,
                    Some(&mut last_line_index),
                ));

                if i > 0 && last_line_index + 1 < item_line_index {
                    nest.push(self.newline());
                }
                nest.push(self.build_statement(statement, comment_inserter));
                nest.push(self.newline());
                last_line_index = statement.end_line_index(self);
            }

            nest.extend(self.pull_comments(
                comment_inserter,
                last_line_index,
                end_line_index,
                true,
                Some(&mut last_line_index),
            ));

            if let Some(result) = &block.result {
                if last_line_index + 1 < result.line_index(self) {
                    nest.push(self.newline());
                }

                nest.push(self.build_expression(result, comment_inserter));
                nest.push(self.newline());

                last_line_index = result.end_line_index(self);
            }

            nest.extend(self.pull_comments(
                comment_inserter,
                last_line_index,
                end_line_index,
                true,
                None,
            ));

            list.push(self.nest(self.trim_list(nest), self.indent));
        }
        list.push(self.token(token::TokenKind::CloseBrace));

        self.list(list)
    }

    pub fn build_turbofish(
        &self,
        turbofish: &Loc<ast::TurbofishInner>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match &**turbofish {
            ast::TurbofishInner::Named(arguments) => self.list([
                self.token(token::TokenKind::PathSeparator),
                self.group(
                    "$<",
                    arguments,
                    token::TokenKind::Comma,
                    token::TokenKind::Gt.as_str(),
                    comment_inserter,
                ),
            ]),
            ast::TurbofishInner::Positional(arguments) => self.list([
                self.token(token::TokenKind::PathSeparator),
                self.group(
                    token::TokenKind::Lt.as_str(),
                    arguments,
                    token::TokenKind::Comma,
                    token::TokenKind::Gt.as_str(),
                    comment_inserter,
                ),
            ]),
        }
    }

    pub fn build_argument_list(
        &self,
        argument_list: &Loc<ast::ArgumentList>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match &**argument_list {
            ast::ArgumentList::Positional(arguments) => self.group(
                token::TokenKind::OpenParen.as_str(),
                arguments,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                comment_inserter,
            ),
            ast::ArgumentList::Named(named_arguments) => self.group(
                "$(",
                named_arguments,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                comment_inserter,
            ),
        }
    }

    pub fn build_named_turbofish(
        &self,
        named_turbofish: &ast::NamedTurbofish,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match named_turbofish {
            ast::NamedTurbofish::Full(name, value) => self.list([
                self.text(format!("{name}: ")),
                self.build_type_expression(value, comment_inserter),
            ]),
            ast::NamedTurbofish::Short(name) => self.text(name.to_string()),
        }
    }

    pub fn build_named_argument(
        &self,
        named_argument: &ast::NamedArgument,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match named_argument {
            ast::NamedArgument::Full(name, current) => self.list([
                self.text(format!("{name}: ")),
                self.build_expression(current, comment_inserter),
            ]),
            ast::NamedArgument::Short(name) => self.text(name.to_string()),
        }
    }

    pub fn build_pattern(
        &self,
        pattern: &Loc<ast::Pattern>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match &**pattern {
            ast::Pattern::Integer(int_literal) => {
                self.text(int_literal.to_string())
            }
            ast::Pattern::Bool(bool_literal) => {
                self.text(bool_literal.to_string())
            }
            ast::Pattern::Bound(name, inner) => self.list([
                self.text(format!("{name} @ ")),
                self.build_pattern(inner, comment_inserter),
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
                comment_inserter,
            ),
            ast::Pattern::Array(elements) => self.group(
                token::TokenKind::OpenBracket.as_str(),
                elements,
                token::TokenKind::Comma,
                token::TokenKind::CloseBracket.as_str(),
                comment_inserter,
            ),
            ast::Pattern::Type(name, argument_pattern) => self.list([
                self.build_path(name),
                self.build_argument_pattern(argument_pattern, comment_inserter),
            ]),
        }
    }

    pub fn build_argument_pattern(
        &self,
        argument_pattern: &Loc<ast::ArgumentPattern>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match &**argument_pattern {
            ast::ArgumentPattern::Named(arguments) => self.group(
                "$(",
                arguments,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                comment_inserter,
            ),
            ast::ArgumentPattern::Positional(tuple) => self.group(
                token::TokenKind::OpenParen.as_str(),
                tuple,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                comment_inserter,
            ),
        }
    }

    pub fn build_named_pattern_argument(
        &self,
        argument: &AstNamedPatternArgument,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match &argument.1 {
            Some(pattern) => self.list([
                self.text(format!("{}: ", argument.0)),
                self.build_pattern(pattern, comment_inserter),
            ]),
            None => self.text(argument.0.to_string()),
        }
    }

    pub fn build_type_expression(
        &self,
        type_expression: &Loc<ast::TypeExpression>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match &**type_expression {
            ast::TypeExpression::TypeSpec(type_spec) => {
                self.build_type_spec(type_spec, comment_inserter)
            }
            ast::TypeExpression::Bool(value) => self.text(value.to_string()),
            ast::TypeExpression::Integer(value) => self.text(value.to_string()),
            // Const generics are always brace-delimited in type position.
            ast::TypeExpression::ConstGeneric(expression) => self.list([
                self.text("{"),
                self.build_expression(expression, comment_inserter),
                self.text("}"),
            ]),
            // Same no-escape lexing as string literals: the parsed
            // value is the exact source text.
            ast::TypeExpression::String(value) => {
                self.text(format!("\"{value}\""))
            }
        }
    }

    pub fn build_type_spec(
        &self,
        type_spec: &Loc<ast::TypeSpec>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match &**type_spec {
            ast::TypeSpec::Tuple(elements) => self.group(
                token::TokenKind::OpenParen.as_str(),
                elements,
                token::TokenKind::Comma,
                token::TokenKind::CloseParen.as_str(),
                comment_inserter,
            ),
            ast::TypeSpec::Array { inner, size } => self.list([
                self.token(token::TokenKind::OpenBracket),
                self.build_type_expression(inner, comment_inserter),
                self.token(token::TokenKind::Semi),
                self.text(" "),
                self.build_type_expression(size, comment_inserter),
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
                        comment_inserter,
                    ));
                }
                self.list(list)
            }
            ast::TypeSpec::Inverted(inner) => self.list([
                self.text("inv "),
                self.build_type_expression(inner, comment_inserter),
            ]),
            ast::TypeSpec::CopyView(inner) => self.list([
                self.text("&"),
                self.build_type_expression(inner, comment_inserter),
            ]),
            ast::TypeSpec::Impl(traits) => {
                let mut list = vec![self.text("impl ")];
                for (i, trait_spec) in traits.iter().enumerate() {
                    if i > 0 {
                        list.push(self.text(" + "));
                    }
                    list.push(
                        self.build_trait_spec(trait_spec, comment_inserter),
                    );
                }
                self.list(list)
            }
            ast::TypeSpec::Wildcard => self.text("_"),
        }
    }

    pub fn build_type_param(
        &self,
        type_param: &Loc<ast::TypeParam>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        match &**type_param {
            ast::TypeParam::TypeName {
                name,
                traits,
                default,
            } => {
                let mut list = vec![self.text(name.to_string())];
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
                        flatten_list.push(
                            self.build_trait_spec(trait_spec, comment_inserter),
                        );
                        nest_list.push(
                            self.build_trait_spec(trait_spec, comment_inserter),
                        );
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
                        self.build_type_expression(default, comment_inserter),
                    ]);
                }
                self.list(list)
            }
            ast::TypeParam::TypeWithMeta {
                meta,
                name,
                default,
            } => {
                let mut list = vec![self.text(format!("#{meta} {name}"))];
                if let Some(default) = default {
                    list.extend([
                        self.text(" = "),
                        self.build_type_expression(default, comment_inserter),
                    ]);
                }
                self.list(list)
            }
        }
    }

    pub fn build_trait_spec(
        &self,
        trait_spec: &Loc<ast::TraitSpec>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        // `Fn(T) -> O` sugar arrives desugared; reprinting it as
        // `Fn<(T), O>` would rewrite the source.
        if trait_spec.paren_syntax {
            return self.unsupported(
                trait_spec,
                "parenthesized trait sugar (`Fn(..) -> ..`)",
            );
        }
        let mut list = vec![self.build_path(&trait_spec.path)];
        if let Some(type_params) = &trait_spec.type_params {
            list.push(self.group(
                token::TokenKind::Lt.as_str(),
                &type_params.inner,
                token::TokenKind::Comma,
                token::TokenKind::Gt.as_str(),
                comment_inserter,
            ));
        }
        self.list(list)
    }

    pub fn build_attribute(
        &self,
        attribute: &Loc<ast::Attribute>,
    ) -> DocumentIdx {
        match &**attribute {
            ast::Attribute::Optimize { .. } => {
                self.unsupported(attribute, "the `#[optimize]` attribute")
            }
            ast::Attribute::NoMangle { all } => self.text(format!(
                "#[no_mangle{}]",
                if *all { "(all)" } else { "" }
            )),
            ast::Attribute::Fsm { .. } => {
                self.unsupported(attribute, "the `#[fsm]` attribute")
            }
            ast::Attribute::Documentation { content } => {
                self.text(format!("///{content}"))
            }
            ast::Attribute::SurferTranslator(_) => self
                .unsupported(attribute, "the `#[surfer_translator]` attribute"),
            ast::Attribute::SpadecParenSugar => {
                self.text("#[spadec_paren_sugar]")
            }
            ast::Attribute::Inline => self.text("#[inline]"),
            ast::Attribute::Deprecated { .. } => {
                self.unsupported(attribute, "the `#[deprecated]` attribute")
            }
            ast::Attribute::VerilogAttrs { .. } => {
                self.unsupported(attribute, "the `#[verilog_attrs]` attribute")
            }
        }
    }

    pub fn build_attribute_list(
        &self,
        attribute_list: &ast::AttributeList,
        always_newline: bool,
    ) -> DocumentIdx {
        // A `///` doc renders as a line comment; in inline or flattenable
        // positions everything after it on the line would be swallowed.
        if !always_newline
            && let Some(doc) = attribute_list.0.iter().find(|attribute| {
                matches!(***attribute, ast::Attribute::Documentation { .. })
            })
        {
            self.record_unsupported(
                doc,
                "documentation on parameters or struct members",
            );
        }
        self.list(match attribute_list.0.len() {
            0 => vec![],
            1 => vec![
                self.build_attribute(&attribute_list.0[0]),
                if always_newline {
                    self.newline()
                } else {
                    self.text(" ")
                },
            ],
            _ => {
                let mut list = vec![];
                for attribute in &attribute_list.0 {
                    list.extend([
                        self.build_attribute(attribute),
                        self.newline(),
                    ]);
                }
                list
            }
        })
    }

    pub fn build_parameter(
        &self,
        parameter: &AstParameter,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        let mut list = vec![self.build_attribute_list(&parameter.0, false)];
        if parameter.1.is_some() {
            list.push(self.text("wire "));
        }
        list.extend([
            self.text(format!("{}: ", parameter.2)),
            self.build_type_spec(&parameter.3, comment_inserter),
        ]);
        self.list(list)
    }

    pub fn build_array_element(
        &self,
        element: &AstArrayElement,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        let mut list = vec![];
        if let Some(label) = &element.0 {
            list.push(self.text(format!("'{label} ")));
        }
        list.push(self.build_expression(&element.1, comment_inserter));
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

    /// Returns a (try, catch) pair of documents for formatting the given
    /// `parameter_list`.
    pub fn build_parameter_list(
        &self,
        parameter_list: &Loc<ast::ParameterList>,
        comment_inserter: &mut CommentInserter,
    ) -> (DocumentIdx, DocumentIdx) {
        let mut try_list = vec![];
        let mut catch_list = vec![];
        if let Some((attributes, wire, amp)) = &parameter_list.self_ {
            let self_doc = |trailing: &str| {
                let mut list =
                    vec![self.build_attribute_list(attributes, false)];
                if wire.is_some() {
                    list.push(self.text("wire "));
                }
                if amp.is_some() {
                    list.push(self.text("&"));
                }
                list.push(self.text(format!("self{trailing}")));
                self.list(list)
            };
            let continues = !parameter_list.args.is_empty();
            try_list.push(self_doc(if continues { ", " } else { "" }));
            catch_list.extend([
                self.newline(),
                self.nest(self_doc(","), self.indent),
            ]);
        }
        let (try_idx, catch_idx) = self.group_raw(
            &parameter_list.args,
            token::TokenKind::Comma,
            comment_inserter,
        );
        try_list.push(try_idx);
        catch_list.push(catch_idx);
        (self.list(try_list), self.list(catch_list))
    }

    fn newline(&self) -> DocumentIdx {
        self.inner.borrow_mut().add(Document::Newline)
    }

    fn text(&self, text: impl Into<String>) -> DocumentIdx {
        self.inner.borrow_mut().add(Document::Text(text.into()))
    }

    fn raw_text(&self, raw_text: impl Into<String>) -> DocumentIdx {
        self.inner.borrow_mut().add(Document::Raw(raw_text.into()))
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

    fn group_raw<'a, B: BuildAsDocument + HasLineNumber + 'a>(
        &self,
        contents: impl IntoIterator<Item = &'a B>,
        between: impl Into<Option<token::TokenKind>>,
        comment_inserter: &mut CommentInserter,
    ) -> (DocumentIdx, DocumentIdx) {
        let between = between.into();

        let mut list = vec![];
        let mut last_line_index = 0;
        for (i, (item, item_line_index, item_end_line_index)) in contents
            .into_iter()
            .map(|item| {
                (
                    item.build(self, comment_inserter),
                    item.line_index(self),
                    item.end_line_index(self),
                )
            })
            .enumerate()
        {
            if i > 0 {
                if let Some(ref between) = between {
                    list.extend([self.token(between.clone()), self.newline()]);
                }
                if last_line_index + 1 < item_line_index {
                    list.push(self.newline());
                }
            }
            list.push(item);
            last_line_index = item_end_line_index;
        }
        let doc_contents = self.list(list);
        let mut nest_list =
            vec![self.newline(), self.nest(doc_contents, self.indent)];
        if matches!(between, Some(token::TokenKind::Comma)) {
            // always trailing comma when nesting a comma group, could
            // overestimate
            nest_list.push(self.token(token::TokenKind::Comma));
        }
        nest_list.push(self.newline());
        // try to flatten, otherwise nest
        (self.flatten(doc_contents), self.list(nest_list))
    }

    fn group<'a, B: BuildAsDocument + HasLineNumber + 'a>(
        &self,
        open: impl Into<String>,
        contents: impl IntoIterator<Item = &'a B>,
        between: impl Into<Option<token::TokenKind>>,
        close: impl Into<String>,
        comment_inserter: &mut CommentInserter,
    ) -> DocumentIdx {
        let open = open.into();
        let close = close.into();

        let (try_body_idx, catch_body_idx) =
            self.group_raw(contents, between, comment_inserter);
        let mut try_list = vec![];
        let mut catch_list = vec![];
        //if let Some(open) = open {
        try_list.push(self.text(open.clone()));
        catch_list.push(self.text(open));
        //}
        try_list.push(try_body_idx);
        catch_list.push(catch_body_idx);
        //if let Some(close) = close {
        try_list.push(self.text(close.clone()));
        catch_list.push(self.text(close));
        //}
        self.try_catch(self.list(try_list), self.list(catch_list))
    }
}
