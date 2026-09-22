//! Things to wrap other things in file ids.
use std::borrow::Borrow;

use base_db::SourceDatabase;
use either::Either;
use span::{AstIdNode, ErasedFileAstId, FileAstId, FileId, SyntaxContext};
use syntax::{AstNode, AstPtr, SyntaxNode, SyntaxNodePtr, SyntaxToken, TextRange, TextSize};

use crate::{
    EditionedFileId, HirFileId, MacroCallId, MacroKind, map_node_range_up,
    map_node_range_up_rooted, span_for_offset,
};

/// `InFile<T>` stores a value of `T` inside a particular file/syntax tree.
///
/// Typical usages are:
///
/// * `InFile<SyntaxNode>` -- syntax node in a file
/// * `InFile<ast::FnDef>` -- ast node in a file
/// * `InFile<TextSize>` -- offset in a file
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash, salsa::SalsaValue)]
pub struct InFileWrapper<FileKind, T> {
    pub file_id: FileKind,
    pub value: T,
}
pub type InFile<'db, T> = InFileWrapper<HirFileId<'db>, T>;
pub type InMacroFile<'db, T> = InFileWrapper<MacroCallId<'db>, T>;
pub type InRealFile<T> = InFileWrapper<EditionedFileId, T>;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub struct FilePositionWrapper<FileKind> {
    pub file_id: FileKind,
    pub offset: TextSize,
}
pub type HirFilePosition<'db> = FilePositionWrapper<HirFileId<'db>>;
pub type MacroFilePosition<'db> = FilePositionWrapper<MacroCallId<'db>>;
pub type FilePosition = FilePositionWrapper<EditionedFileId>;

impl FilePosition {
    #[inline]
    pub fn into_file_id(self, db: &dyn SourceDatabase) -> FilePositionWrapper<FileId> {
        FilePositionWrapper { file_id: self.file_id.file_id(db), offset: self.offset }
    }
}

impl<'db> From<FileRange> for HirFileRange<'db> {
    fn from(value: FileRange) -> Self {
        HirFileRange { file_id: value.file_id.into(), range: value.range }
    }
}

impl<'db> From<FilePosition> for HirFilePosition<'db> {
    fn from(value: FilePosition) -> Self {
        HirFilePosition { file_id: value.file_id.into(), offset: value.offset }
    }
}

impl<'db> HirFileRange<'db> {
    pub fn file_range(self) -> Option<FileRange> {
        Some(FileRange { file_id: self.file_id.file_id()?, range: self.range })
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub struct FileRangeWrapper<FileKind> {
    pub file_id: FileKind,
    pub range: TextRange,
}
pub type HirFileRange<'db> = FileRangeWrapper<HirFileId<'db>>;
pub type MacroFileRange<'db> = FileRangeWrapper<MacroCallId<'db>>;
pub type FileRange = FileRangeWrapper<EditionedFileId>;

impl FileRange {
    #[inline]
    pub fn into_file_id(self, db: &dyn SourceDatabase) -> FileRangeWrapper<FileId> {
        FileRangeWrapper { file_id: self.file_id.file_id(db), range: self.range }
    }

    #[inline]
    pub fn file_text(self, db: &dyn SourceDatabase) -> &triomphe::Arc<str> {
        db.file_text(self.file_id.file_id(db)).text(db)
    }

    #[inline]
    pub fn text(self, db: &dyn SourceDatabase) -> &str {
        &self.file_text(db)[self.range]
    }
}

/// `AstId` points to an AST node in any file.
///
/// It is stable across reparses, and can be used as salsa key/value.
pub type AstId<'db, N> = crate::InFile<'db, FileAstId<N>>;

impl<'db, N: AstNode> AstId<'db, N> {
    pub fn to_node(&self, db: &dyn SourceDatabase) -> N {
        self.to_ptr(db).to_node(&self.file_id.parse_or_expand(db))
    }
    pub fn to_range(&self, db: &dyn SourceDatabase) -> TextRange {
        self.to_ptr(db).text_range()
    }
    pub fn to_in_file_node(&self, db: &dyn SourceDatabase) -> crate::InFile<'db, N> {
        crate::InFile::new(self.file_id, self.to_ptr(db).to_node(&self.file_id.parse_or_expand(db)))
    }
    pub fn to_ptr(&self, db: &dyn SourceDatabase) -> AstPtr<N> {
        self.file_id.ast_id_map(db).get(self.value)
    }
    pub fn erase(&self) -> ErasedAstId<'db> {
        crate::InFile::new(self.file_id, self.value.erase())
    }
    #[inline]
    pub fn upcast<M: AstIdNode>(self) -> AstId<'db, M>
    where
        N: Into<M>,
    {
        self.map(|it| it.upcast())
    }
}

pub type ErasedAstId<'db> = crate::InFile<'db, ErasedFileAstId>;

impl<'db> ErasedAstId<'db> {
    pub fn to_range(&self, db: &dyn SourceDatabase) -> TextRange {
        self.to_ptr(db).text_range()
    }
    pub fn to_ptr(&self, db: &dyn SourceDatabase) -> SyntaxNodePtr {
        self.file_id.ast_id_map(db).get_erased(self.value)
    }
}

impl<FileKind, N: AstNode> InFileWrapper<FileKind, AstPtr<N>> {
    #[inline]
    pub fn upcast<M: AstNode>(self) -> InFileWrapper<FileKind, AstPtr<M>>
    where
        N: Into<M>,
    {
        self.map(|it| it.upcast())
    }
}

impl<FileKind, T> InFileWrapper<FileKind, T> {
    pub fn new(file_id: FileKind, value: T) -> Self {
        Self { file_id, value }
    }

    pub fn map<F: FnOnce(T) -> U, U>(self, f: F) -> InFileWrapper<FileKind, U> {
        InFileWrapper::new(self.file_id, f(self.value))
    }
}

impl<FileKind: Copy, T> InFileWrapper<FileKind, T> {
    pub fn with_value<U>(&self, value: U) -> InFileWrapper<FileKind, U> {
        InFileWrapper::new(self.file_id, value)
    }

    pub fn as_ref(&self) -> InFileWrapper<FileKind, &T> {
        self.with_value(&self.value)
    }

    pub fn borrow<U>(&self) -> InFileWrapper<FileKind, &U>
    where
        T: Borrow<U>,
    {
        self.with_value(self.value.borrow())
    }
}

impl<FileKind: Copy, T: Clone> InFileWrapper<FileKind, &T> {
    pub fn cloned(&self) -> InFileWrapper<FileKind, T> {
        self.with_value(self.value.clone())
    }
}

impl<'db, T> From<InMacroFile<'db, T>> for InFile<'db, T> {
    fn from(InMacroFile { file_id, value }: InMacroFile<'db, T>) -> Self {
        InFile { file_id: file_id.into(), value }
    }
}

impl<'db, T> From<InRealFile<T>> for InFile<'db, T> {
    fn from(InRealFile { file_id, value }: InRealFile<T>) -> Self {
        InFile { file_id: file_id.into(), value }
    }
}

// region:transpose impls

impl<FileKind, T> InFileWrapper<FileKind, Option<T>> {
    pub fn transpose(self) -> Option<InFileWrapper<FileKind, T>> {
        Some(InFileWrapper::new(self.file_id, self.value?))
    }
}

impl<FileKind, L, R> InFileWrapper<FileKind, Either<L, R>> {
    pub fn transpose(self) -> Either<InFileWrapper<FileKind, L>, InFileWrapper<FileKind, R>> {
        match self.value {
            Either::Left(l) => Either::Left(InFileWrapper::new(self.file_id, l)),
            Either::Right(r) => Either::Right(InFileWrapper::new(self.file_id, r)),
        }
    }
}

// endregion:transpose impls

trait FileIdToSyntax: Copy {
    fn file_syntax(self, db: &dyn SourceDatabase) -> SyntaxNode;
}

impl FileIdToSyntax for EditionedFileId {
    fn file_syntax(self, db: &dyn SourceDatabase) -> SyntaxNode {
        self.parse(db).syntax_node()
    }
}
impl FileIdToSyntax for MacroCallId<'_> {
    fn file_syntax(self, db: &dyn SourceDatabase) -> SyntaxNode {
        self.parse_macro_expansion(db).value.0.syntax_node()
    }
}
impl FileIdToSyntax for HirFileId<'_> {
    fn file_syntax(self, db: &dyn SourceDatabase) -> SyntaxNode {
        self.parse_or_expand(db)
    }
}

#[allow(private_bounds)]
impl<FileId: FileIdToSyntax, T> InFileWrapper<FileId, T> {
    pub fn file_syntax(&self, db: &dyn SourceDatabase) -> SyntaxNode {
        FileIdToSyntax::file_syntax(self.file_id, db)
    }
}

#[allow(private_bounds)]
impl<FileId: FileIdToSyntax, N: AstNode> InFileWrapper<FileId, AstPtr<N>> {
    pub fn to_node(&self, db: &dyn SourceDatabase) -> N {
        self.value.to_node(&self.file_syntax(db))
    }
}

impl<FileId: Copy, N: AstNode> InFileWrapper<FileId, N> {
    pub fn syntax(&self) -> InFileWrapper<FileId, &SyntaxNode> {
        self.with_value(self.value.syntax())
    }
    pub fn node_file_range(&self) -> FileRangeWrapper<FileId> {
        FileRangeWrapper { file_id: self.file_id, range: self.value.syntax().text_range() }
    }
}

impl<FileId: Copy, N: AstNode> InFileWrapper<FileId, &N> {
    // unfortunately `syntax` collides with the impl above, because `&_` is fundamental
    pub fn syntax_ref(&self) -> InFileWrapper<FileId, &SyntaxNode> {
        self.with_value(self.value.syntax())
    }
}

// region:specific impls
impl<FileId: Copy, SN: Borrow<SyntaxNode>> InFileWrapper<FileId, SN> {
    pub fn file_range(&self) -> FileRangeWrapper<FileId> {
        FileRangeWrapper { file_id: self.file_id, range: self.value.borrow().text_range() }
    }
}

impl<'db, SN: Borrow<SyntaxNode>> InFile<'db, SN> {
    pub fn parent_ancestors_with_macros(
        self,
        db: &'db dyn SourceDatabase,
    ) -> impl Iterator<Item = InFile<'db, SyntaxNode>> {
        let succ = move |node: &InFile<'db, SyntaxNode>| match node.value.parent() {
            Some(parent) => Some(node.with_value(parent)),
            None => node
                .file_id
                .macro_file()?
                .loc(db)
                .to_node_item(db)
                .syntax()
                .cloned()
                .map(|node| node.parent())
                .transpose(),
        };
        std::iter::successors(succ(&self.borrow().cloned()), succ)
    }

    pub fn ancestors_with_macros(
        self,
        db: &'db dyn SourceDatabase,
    ) -> impl Iterator<Item = InFile<'db, SyntaxNode>> {
        let succ = move |node: &InFile<'db, SyntaxNode>| match node.value.parent() {
            Some(parent) => Some(node.with_value(parent)),
            None => node
                .file_id
                .macro_file()?
                .loc(db)
                .to_node_item(db)
                .syntax()
                .cloned()
                .map(|node| node.parent())
                .transpose(),
        };
        std::iter::successors(Some(self.borrow().cloned()), succ)
    }

    pub fn kind(&self) -> parser::SyntaxKind {
        self.value.borrow().kind()
    }

    pub fn text_range(&self) -> TextRange {
        self.value.borrow().text_range()
    }

    /// Falls back to the macro call range if the node cannot be mapped up fully.
    ///
    /// For attributes and derives, this will point back to the attribute only.
    /// For the entire item use `InFile::original_file_range_full`.
    pub fn original_file_range_rooted(self, db: &dyn SourceDatabase) -> FileRange {
        self.borrow().map(SyntaxNode::text_range).original_node_file_range_rooted(db)
    }

    /// Falls back to the macro call range if the node cannot be mapped up fully.
    pub fn original_file_range_with_macro_call_input(self, db: &dyn SourceDatabase) -> FileRange {
        self.borrow().map(SyntaxNode::text_range).original_node_file_range_with_macro_call_input(db)
    }

    pub fn original_syntax_node_rooted(
        self,
        db: &dyn SourceDatabase,
    ) -> Option<InRealFile<SyntaxNode>> {
        // This kind of upmapping can only be achieved in attribute expanded files,
        // as we don't have node inputs otherwise and therefore can't find an `N` node in the input
        let file_id = match self.file_id {
            HirFileId::FileId(file_id) => {
                return Some(InRealFile { file_id, value: self.value.borrow().clone() });
            }
            HirFileId::MacroFile(m)
                if matches!(m.kind(db), MacroKind::Attr | MacroKind::AttrBuiltIn) =>
            {
                m
            }
            _ => return None,
        };

        let FileRange { file_id: editioned_file_id, range } = map_node_range_up_rooted(
            db,
            file_id.expansion_span_map(db),
            self.value.borrow().text_range(),
        )?;

        let kind = self.kind();
        let value = editioned_file_id
            .parse(db)
            .syntax_node()
            .covering_element(range)
            .ancestors()
            .take_while(|it| it.text_range() == range)
            .find(|it| it.kind() == kind)?;
        Some(InRealFile::new(editioned_file_id, value))
    }
}

impl InFile<'_, &SyntaxNode> {
    /// Attempts to map the syntax node back up its macro calls.
    pub fn original_file_range_opt(
        self,
        db: &dyn SourceDatabase,
    ) -> Option<(FileRange, SyntaxContext)> {
        self.borrow().map(SyntaxNode::text_range).original_node_file_range_opt(db)
    }
}

impl<'db> InMacroFile<'db, SyntaxToken> {
    pub fn upmap_once(
        self,
        db: &'db dyn SourceDatabase,
    ) -> InFile<'db, smallvec::SmallVec<[TextRange; 1]>> {
        self.file_id.expansion_info(db).map_range_up_once(db, self.value.text_range())
    }
}

impl<'db> InFile<'db, SyntaxToken> {
    /// Falls back to the macro call range if the node cannot be mapped up fully.
    pub fn original_file_range(self, db: &dyn SourceDatabase) -> FileRange {
        match self.file_id {
            HirFileId::FileId(file_id) => FileRange { file_id, range: self.value.text_range() },
            HirFileId::MacroFile(mac_file) => {
                let (range, ctxt) = span_for_offset(
                    db,
                    mac_file.expansion_span_map(db),
                    self.value.text_range().start(),
                );

                // FIXME: Figure out an API that makes proper use of ctx, this only exists to
                // keep pre-token map rewrite behaviour.
                if ctxt.is_root() {
                    return range;
                }

                // Fall back to whole macro call.
                let loc = mac_file.loc(db);
                loc.kind.original_call_range(db, loc.krate)
            }
        }
    }

    /// Attempts to map the syntax node back up its macro calls.
    pub fn original_file_range_opt(self, db: &dyn SourceDatabase) -> Option<FileRange> {
        match self.file_id {
            HirFileId::FileId(file_id) => {
                Some(FileRange { file_id, range: self.value.text_range() })
            }
            HirFileId::MacroFile(mac_file) => {
                let (range, ctxt) = span_for_offset(
                    db,
                    mac_file.expansion_span_map(db),
                    self.value.text_range().start(),
                );

                // FIXME: Figure out an API that makes proper use of ctx, this only exists to
                // keep pre-token map rewrite behaviour.
                if ctxt.is_root() { Some(range) } else { None }
            }
        }
    }
}

impl InMacroFile<'_, TextSize> {
    pub fn original_file_range(self, db: &dyn SourceDatabase) -> (FileRange, SyntaxContext) {
        span_for_offset(db, self.file_id.expansion_span_map(db), self.value)
    }
}

impl InFile<'_, TextRange> {
    pub fn original_node_file_range(self, db: &dyn SourceDatabase) -> (FileRange, SyntaxContext) {
        match self.file_id {
            HirFileId::FileId(file_id) => {
                (FileRange { file_id, range: self.value }, SyntaxContext::root(file_id.edition(db)))
            }
            HirFileId::MacroFile(mac_file) => {
                match map_node_range_up(db, mac_file.expansion_span_map(db), self.value) {
                    Some(it) => it,
                    None => {
                        let loc = mac_file.loc(db);
                        (
                            loc.kind.original_call_range(db, loc.krate),
                            SyntaxContext::root(loc.def.edition),
                        )
                    }
                }
            }
        }
    }

    pub fn original_node_file_range_rooted(self, db: &dyn SourceDatabase) -> FileRange {
        match self.file_id {
            HirFileId::FileId(file_id) => FileRange { file_id, range: self.value },
            HirFileId::MacroFile(mac_file) => {
                match map_node_range_up_rooted(db, mac_file.expansion_span_map(db), self.value) {
                    Some(it) => it,
                    _ => {
                        let loc = mac_file.loc(db);
                        loc.kind.original_call_range(db, loc.krate)
                    }
                }
            }
        }
    }

    pub fn original_node_file_range_with_macro_call_input(
        self,
        db: &dyn SourceDatabase,
    ) -> FileRange {
        match self.file_id {
            HirFileId::FileId(file_id) => FileRange { file_id, range: self.value },
            HirFileId::MacroFile(mac_file) => {
                match map_node_range_up_rooted(db, mac_file.expansion_span_map(db), self.value) {
                    Some(it) => it,
                    _ => {
                        let loc = mac_file.loc(db);
                        loc.kind.original_call_range_with_input(db)
                    }
                }
            }
        }
    }

    pub fn original_node_file_range_opt(
        self,
        db: &dyn SourceDatabase,
    ) -> Option<(FileRange, SyntaxContext)> {
        match self.file_id {
            HirFileId::FileId(file_id) => Some((
                FileRange { file_id, range: self.value },
                SyntaxContext::root(file_id.edition(db)),
            )),
            HirFileId::MacroFile(mac_file) => {
                map_node_range_up(db, mac_file.expansion_span_map(db), self.value)
            }
        }
    }

    pub fn original_node_file_range_rooted_opt(self, db: &dyn SourceDatabase) -> Option<FileRange> {
        match self.file_id {
            HirFileId::FileId(file_id) => Some(FileRange { file_id, range: self.value }),
            HirFileId::MacroFile(mac_file) => {
                map_node_range_up_rooted(db, mac_file.expansion_span_map(db), self.value)
            }
        }
    }
}

impl<N: AstNode> InFile<'_, N> {
    pub fn original_ast_node_rooted(self, db: &dyn SourceDatabase) -> Option<InRealFile<N>> {
        // This kind of upmapping can only be achieved in attribute expanded files,
        // as we don't have node inputs otherwise and therefore can't find an `N` node in the input
        let file_id = match self.file_id {
            HirFileId::FileId(file_id) => {
                return Some(InRealFile { file_id, value: self.value });
            }
            HirFileId::MacroFile(m) => m,
        };
        if !matches!(file_id.kind(db), MacroKind::Attr | MacroKind::AttrBuiltIn) {
            return None;
        }

        let FileRange { file_id: editioned_file_id, range } = map_node_range_up_rooted(
            db,
            file_id.expansion_span_map(db),
            self.value.syntax().text_range(),
        )?;

        // FIXME: This heuristic is brittle and with the right macro may select completely unrelated nodes?
        let anc = editioned_file_id.parse(db).syntax_node().covering_element(range);
        let value = anc.ancestors().find_map(N::cast)?;
        Some(InRealFile::new(editioned_file_id, value))
    }
}

impl<'db, T> InFile<'db, T> {
    pub fn into_real_file(self) -> Result<InRealFile<T>, InFile<'db, T>> {
        match self.file_id {
            HirFileId::FileId(file_id) => Ok(InRealFile { file_id, value: self.value }),
            HirFileId::MacroFile(_) => Err(self),
        }
    }
}
