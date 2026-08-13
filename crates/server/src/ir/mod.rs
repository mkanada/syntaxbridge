//! Minimal intermediate representation bridging C++ analysis
//! (`function_catalog`/`lower::cpp`) and Dart emission (`emit::dart`).
//!
//! Grows one degrau of `examples/` at a time — see
//! `docs/plans/primeiro-corte-e01-e03.md` §7. Do not add a variant here
//! before some degrau actually needs it (AGENTS.md: no premature
//! abstraction). E01 needed `Type::{Int, Void}`, `Ref`, `Binary`,
//! `IntLiteral` and `Return`. E02 (control flow) added `Type::{Bool,
//! Double}`, the comparison/arithmetic `BinaryOp` variants, `UnaryOp`,
//! `Expr::{BoolLiteral, Call, Unary}`, and `Stmt::{If, While, For, VarDecl,
//! Assign, ExprStmt}`. E03 (aggregates) added `Record`/`Field`,
//! `Type::Record`, `Expr::{FieldAccess, RecordConstruct}` and
//! `Stmt::FieldAssign`.
//!
//! Two invariants hold from the first commit, per §5 of that plan
//! (retrofitting either later is expensive):
//! - **Rastreabilidade**: every node carries its C++ [`Origin`].
//! - **Silêncio é proibido**: any construct the lowering doesn't recognize
//!   becomes an `Unsupported` node (with origin and reason) instead of being
//!   dropped — see `Type::Unsupported`, `Expr::Unsupported`,
//!   `Stmt::Unsupported`.

use serde::{Deserialize, Serialize};

/// Where a piece of IR came from in the original C++.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Origin {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Type {
    Int,
    Bool,
    Double,
    Void,
    /// A `struct`/`class` type — `usr` is the join key to the record's own
    /// declaration (`Module::records`) and to `type_catalog`'s catalog;
    /// `name` rides along so `emit::dart` can print a type annotation
    /// without needing the whole `Module` in scope everywhere a `Type` is.
    Record {
        usr: String,
        name: String,
    },
    /// A C++ type the lowering doesn't represent yet, carrying its spelling
    /// (e.g. `"float"`) so the reason is legible instead of silently
    /// dropped.
    Unsupported(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    /// Emitted as `~/` or `/` depending on the node's own [`Type`] (`Int` →
    /// truncating, otherwise real division) — E02's armadilha
    /// (`docs/plans/primeiro-corte-e01-e03.md` §7 PR4): C++ `int / int`
    /// truncates, Dart `/` never does.
    Div,
    Mod,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaryOp {
    Neg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Expr {
    IntLiteral {
        value: i64,
        origin: Origin,
    },
    BoolLiteral {
        value: bool,
        origin: Origin,
    },
    Ref {
        name: String,
        ty: Type,
        origin: Origin,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        ty: Type,
        origin: Origin,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        ty: Type,
        origin: Origin,
    },
    /// An implicit numeric promotion C++ inserted on `operand` (currently
    /// only `int` → `double`, from the usual arithmetic conversions or an
    /// `int` initializing/assigning to a `double`) — `lower::cpp` must keep
    /// this explicit rather than discarding it as sugar, since Dart (unlike
    /// C++) never implicitly widens an `int` *expression* to `double`
    /// (only integer *literals* coerce). `emit::dart` turns this into
    /// `<operand>.toDouble()`.
    Convert {
        operand: Box<Expr>,
        ty: Type,
        origin: Origin,
    },
    /// A call to another free function — `callee_usr` is the join key back
    /// to `function_catalog`'s catalog (and, for a recursive/self call, to
    /// this very `Function`).
    Call {
        callee_usr: String,
        callee_name: String,
        args: Vec<Expr>,
        ty: Type,
        origin: Origin,
    },
    /// `p.x` — `target` is usually a `Ref`, but kept as a boxed `Expr`
    /// rather than a bare variable name since a field access can itself be
    /// the target of another field access (`a.b.c`), even though no
    /// E01–E03 fixture nests one yet.
    FieldAccess {
        target: Box<Expr>,
        field: String,
        ty: Type,
        origin: Origin,
    },
    /// Constructing a record value from its field values, in declaration
    /// order — used both for an explicit aggregate construction in the
    /// source and for the implicit copy `lower::cpp` inserts at the top of
    /// a function for every by-value `Record` parameter (E03's armadilha:
    /// C++ copies a struct passed by value, Dart passes the reference —
    /// see `examples/E03-struct-pod/NOTES.md`).
    RecordConstruct {
        type_usr: String,
        type_name: String,
        fields: Vec<(String, Expr)>,
        origin: Origin,
    },
    Unsupported {
        reason: String,
        origin: Origin,
    },
}

impl Expr {
    pub fn origin(&self) -> &Origin {
        match self {
            Self::IntLiteral { origin, .. }
            | Self::BoolLiteral { origin, .. }
            | Self::Ref { origin, .. }
            | Self::Binary { origin, .. }
            | Self::Unary { origin, .. }
            | Self::Convert { origin, .. }
            | Self::Call { origin, .. }
            | Self::FieldAccess { origin, .. }
            | Self::RecordConstruct { origin, .. }
            | Self::Unsupported { origin, .. } => origin,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Stmt {
    Return {
        value: Option<Expr>,
        origin: Origin,
    },
    VarDecl {
        name: String,
        ty: Type,
        init: Option<Expr>,
        origin: Origin,
    },
    /// Assignment to a simple local variable (`name = value;`) — assigning
    /// through an index (`a[i] = x;`) is out of scope until E10.
    Assign {
        name: String,
        value: Expr,
        origin: Origin,
    },
    /// `p.x = value;` — assignment through a field, as opposed to `Assign`'s
    /// simple-variable target.
    FieldAssign {
        target: Expr,
        field: String,
        value: Expr,
        origin: Origin,
    },
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        /// Empty when there is no `else` — kept as `Vec` rather than
        /// `Option<Vec<_>>` since "no else" and "empty else block" emit
        /// identically anyway.
        else_branch: Vec<Stmt>,
        origin: Origin,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
        origin: Origin,
    },
    For {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        increment: Option<Box<Stmt>>,
        body: Vec<Stmt>,
        origin: Origin,
    },
    /// A bare expression evaluated for its side effect (e.g. a call whose
    /// result is discarded) — not yet produced by any E01–E03 fixture, but
    /// listed in the plan's PR4 scope as a statement kind the IR needs.
    ExprStmt {
        expr: Expr,
        origin: Origin,
    },
    Unsupported {
        reason: String,
        origin: Origin,
    },
}

impl Stmt {
    pub fn origin(&self) -> &Origin {
        match self {
            Self::Return { origin, .. }
            | Self::VarDecl { origin, .. }
            | Self::Assign { origin, .. }
            | Self::FieldAssign { origin, .. }
            | Self::If { origin, .. }
            | Self::While { origin, .. }
            | Self::For { origin, .. }
            | Self::ExprStmt { origin, .. }
            | Self::Unsupported { origin, .. } => origin,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    /// `usr` of the corresponding `function_catalog::FunctionDeclaration` —
    /// the join key between the catalog and this IR.
    pub usr: String,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Vec<Stmt>,
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    /// `usr` of the corresponding `type_catalog::TypeDeclaration` — the join
    /// key between the type catalog and this IR, and what `Type::Record`
    /// points back to.
    pub usr: String,
    /// In declaration order — field order is part of the type's identity
    /// for `RecordConstruct`'s positional-constructor emission.
    pub fields: Vec<Field>,
    pub origin: Origin,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub functions: Vec<Function>,
    pub records: Vec<Record>,
}
