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
//! `Stmt::FieldAssign`. E04 (classes with encapsulation) added
//! `Record::{static_fields, constructors, methods}`, `Method`,
//! `Constructor`, `Expr::This`, `Expr::Call::target` (a method call's
//! receiver, `None` for a free function) and `Expr::ConstructorCall` (a
//! real, user-bodied constructor call — distinct from `RecordConstruct`,
//! which stays the E03 aggregate/clone shape for records with no declared
//! constructor of their own).
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
    /// `std::string`/`std::basic_string<char, ...>` — E05's library adapter,
    /// not a `Record`: it's never `lower_record`'d (its fields are libstdc++
    /// internals, not something a Dart class should expose), and its methods
    /// go through their own translation instead of `Record::methods` (see
    /// `Expr::StringByteLength` for why `.size()` in particular can't be a
    /// plain `FieldAccess`/`Call`).
    Str,
    /// `std::vector<T>` — the other half of E05's library adapter, mapped to
    /// Dart's `List<T>`. Only `T = Int` is exercised by any exemplo so far;
    /// the element type is still carried generally (not hardcoded to `Int`)
    /// so a future degrau that needs `vector<double>`/`vector<Record>`
    /// doesn't have to revisit this variant's shape.
    List(Box<Type>),
    /// A Dart record type synthesized as a bridge for C++'s "out parameter"
    /// idiom (`void f(int &a, int &b)`) — E10 flagged the idea and
    /// deliberately didn't build it ("nenhum fixture força essa
    /// complexidade ainda"); E13's `Fraction::Reduce(int&, int&)` does.
    /// Never produced from a C++ type directly (Dart has no reference types
    /// to represent otherwise) — only synthesized by `lower::cpp`'s own
    /// out-param bridge as the new return type of a `void`-returning
    /// function/method that has at least one non-`const` scalar reference
    /// parameter, one slot per such parameter, in parameter order.
    Tuple(Vec<Type>),
    /// `T*` where `T` is itself a type this IR already represents
    /// (`Record`/`Str`/`List`) — `mapping::pointer_options_for`'s
    /// `"referencia-anulavel"` case (`docs/mapping-solver-cases.md` A10):
    /// C++'s static type system already guarantees such a pointer, whatever
    /// it holds at runtime, is always either null or an object whose
    /// dynamic type is `T` or one of `T`'s subtypes — a finite, statically
    /// known set, the same guarantee Dart's own single-reference
    /// polymorphism already relies on. Maps directly to Dart's `T?`.
    /// `lower::cpp::lower_type` only ever produces this for a pointee it
    /// can itself represent; a pointer to anything else (`void`, a scalar,
    /// or an already-`Unsupported` pointee) stays `Type::Unsupported` —
    /// `mapping::pointer_options_for`'s `"ponte-dart-ffi"` case — since
    /// nothing rules out array/pointer-arithmetic use for those.
    Nullable(Box<Type>),
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
    /// A literal written with a decimal point/exponent in the source
    /// (`10.0`, not an integer literal later promoted) — E01–E03 never
    /// needed this: every `double` value they handle either arrives as a
    /// parameter or comes from `Convert`'s int-to-double promotion. E04
    /// surfaces it (`ContaBancaria a(10.0);`), so it earns its own node
    /// instead of overloading `IntLiteral`, which would put an `f64`-typed
    /// hazard behind an `i64` field name.
    DoubleLiteral {
        value: f64,
        origin: Origin,
    },
    BoolLiteral {
        value: bool,
        origin: Origin,
    },
    /// A `std::string`-typed literal (`"Ola, "` — a C `const char*`/array in
    /// the raw C++ type, but always used as a `std::string` operand in every
    /// exemplo that reaches this node, so it's normalized to `Type::Str` at
    /// lowering time rather than carrying the raw C-string type). E05.
    StringLiteral {
        value: String,
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
    /// A call to a free function or a method — `callee_usr` is the join key
    /// back to `function_catalog`'s catalog (and, for a recursive/self call,
    /// to this very `Function`/`Method`). `target` is the receiver: `None`
    /// for a free function call; `Some(Box::new(Expr::This { .. }))` for a
    /// method called without an explicit receiver from inside another
    /// method (`emit::dart` omits the receiver for that case, same as it
    /// omits `this.` for a bare field access); `Some(other)` for a method
    /// called on an explicit object (`obj.method(args)`).
    Call {
        target: Option<Box<Expr>>,
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
    /// A call to one of a record's own (non-copy, non-move) constructors —
    /// `ClassName(args)` for the primary constructor, `ClassName.ctorN(args)`
    /// for every subsequent one, in declaration order (E04's armadilha:
    /// Dart has no signature-based constructor overloading, so a class with
    /// more than one constructor needs the rest named — mechanically, by
    /// ordinal, rather than guessed from intent; see
    /// `examples/E04-classe-com-encapsulamento/NOTES.md`).
    /// `constructor_index` is that ordinal (`0` = primary), computed once in
    /// `lower::cpp` from the same declaration-order walk that numbers
    /// `Record::constructors`, so the two never disagree.
    ConstructorCall {
        type_usr: String,
        type_name: String,
        constructor_index: usize,
        args: Vec<Expr>,
        origin: Origin,
    },
    /// `this` — only ever appears as (part of) another expression: the
    /// target of a `FieldAccess`/`FieldAssign` for an implicit member access,
    /// or the target of a `Call` for a method invoked without an explicit
    /// receiver. `emit::dart` recognizes this shape and omits the receiver
    /// entirely, matching how Dart (like C++) never requires `this.` to
    /// reach a class's own members.
    This {
        ty: Type,
        origin: Origin,
    },
    /// `valores[i]` — `std::vector::operator[]`, the only indexing E05
    /// exercises. Maps directly to Dart's native `target[index]`, which has
    /// the same read semantics, so unlike `.size()` this needs no bridge —
    /// only its own node because `Call`'s shape (`callee_name(args)`) can't
    /// print index syntax.
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
        ty: Type,
        origin: Origin,
    },
    /// `std::string::size()`/`length()` — E05's armadilha
    /// (`examples/E05-biblioteca-padrao/NOTES.md`): C++ counts UTF-8
    /// *bytes*, Dart's `String.length` counts UTF-16 *code units*, and the
    /// two disagree on any non-ASCII content. Not a plain `FieldAccess`
    /// (`.length` alone would be the Dart-native but wrong answer) or `Call`
    /// (no method by this name exists on Dart's `String`) — its own node so
    /// `emit::dart` can print the bridge (`utf8.encode(target).length`,
    /// `dart:convert`) instead.
    StringByteLength {
        target: Box<Expr>,
        origin: Origin,
    },
    /// `(a, b)` — a Dart record value. Only ever synthesized by
    /// `lower::cpp`'s out-param bridge (see `Type::Tuple`), as the trailing
    /// return value of a bridged function/method's body — never lowered
    /// from a C++ cursor directly.
    Tuple {
        values: Vec<Expr>,
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
            | Self::DoubleLiteral { origin, .. }
            | Self::BoolLiteral { origin, .. }
            | Self::StringLiteral { origin, .. }
            | Self::Ref { origin, .. }
            | Self::Binary { origin, .. }
            | Self::Unary { origin, .. }
            | Self::Convert { origin, .. }
            | Self::Call { origin, .. }
            | Self::FieldAccess { origin, .. }
            | Self::RecordConstruct { origin, .. }
            | Self::ConstructorCall { origin, .. }
            | Self::This { origin, .. }
            | Self::Index { origin, .. }
            | Self::StringByteLength { origin, .. }
            | Self::Tuple { origin, .. }
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
    /// `throw value;` (E12) — maps directly to Dart's own `throw`, which
    /// (like C++) can throw any object, not just a designated exception
    /// hierarchy.
    Throw {
        value: Expr,
        origin: Origin,
    },
    /// `try { ... } catch (T name) { ... }` (E12) — a single catch clause;
    /// C++'s multiple-`catch`/catch-all (`catch (...)`) aren't lowered yet,
    /// since no fixture forces either. Maps to Dart's `on T catch (name)`.
    TryCatch {
        try_body: Vec<Stmt>,
        catch_type: Type,
        catch_var: String,
        catch_body: Vec<Stmt>,
        origin: Origin,
    },
    /// A block whose end runs `finally_body` no matter how it's left —
    /// never lowered directly from a C++ cursor (C++ has no `finally`
    /// keyword), but *synthesized* by `lower::cpp::apply_raii_scope_guards`
    /// to stand in for RAII (E12's own armadilha): C++ runs a local's
    /// destructor deterministically at scope exit, and Dart's `try`/
    /// `finally` is the only construct that runs code unconditionally at
    /// block exit, so a local of a type with a real destructor becomes one
    /// of these instead of a plain declaration once lowering is done.
    TryFinally {
        try_body: Vec<Stmt>,
        finally_body: Vec<Stmt>,
        origin: Origin,
    },
    /// `(a, b) = f(...);` — Dart's record-destructuring assignment. Only
    /// ever synthesized by `lower::cpp`'s out-param bridge (see
    /// `Type::Tuple`) at a call site whose callee was itself bridged:
    /// `value` is the (now tuple-typed) call, `targets` the original
    /// by-reference argument expressions, each receiving its matching
    /// tuple slot back, in order.
    TupleAssign {
        targets: Vec<Expr>,
        value: Expr,
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
            | Self::Throw { origin, .. }
            | Self::TryCatch { origin, .. }
            | Self::TryFinally { origin, .. }
            | Self::TupleAssign { origin, .. }
            | Self::Unsupported { origin, .. } => origin,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    /// A C++ default argument (`int passo = 1`, E07) — maps directly to a
    /// Dart optional parameter with the same default, a genuine 1:1
    /// mapping (unlike overloading, which has no Dart equivalent at all).
    /// `None` for an ordinary required parameter.
    pub default_value: Option<Expr>,
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
    /// Already the *Dart* name — a private/protected C++ member (E04's
    /// visibility requirement) gets its leading `_` baked in here, at the
    /// same place every reference to the field resolves its name from
    /// (`lower::cpp::dart_member_name`), so a field's declaration and every
    /// access of it can never disagree on whether it's private.
    pub name: String,
    pub ty: Type,
}

/// An instance or static method — a constructor is
/// [`Constructor`], not a `Method` (its return type isn't a real Dart type,
/// and E04's multiple-constructor armadilha needs its own identity
/// scheme, see `Expr::ConstructorCall`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Method {
    pub name: String,
    /// `usr` of the corresponding `function_catalog::FunctionDeclaration`.
    pub usr: String,
    pub params: Vec<Param>,
    pub return_type: Type,
    /// `None` for a pure virtual method (`virtual T f() = 0;`) — E06's
    /// abstract-method case. A record with at least one such method emits
    /// as `abstract class` (`emit::dart` derives this from the method list
    /// itself rather than a separate `Record` flag, so the two can never
    /// disagree); the method itself emits with no body
    /// (`T f();`, Dart's own abstract-member syntax).
    pub body: Option<Vec<Stmt>>,
    pub is_static: bool,
    /// Whether this method's C++ declaration overrides a base class's
    /// virtual method (`clang_getOverriddenCursors` — not inferred by name
    /// matching, which could be fooled by an unrelated method that happens
    /// to share a name with a base member). Dart doesn't require `@override`
    /// to compile, but this project's own `docs/plans/conversao-guiada-por-exemplos.md`
    /// §6 (E06) calls it out as something this degrau has to produce for
    /// real, not skip as cosmetic.
    pub is_override: bool,
    pub origin: Origin,
}

/// One of a record's own constructors, in declaration order — see
/// `Expr::ConstructorCall` for why order (not name) is the identity that
/// matters. Never a compiler-generated copy/move constructor: `lower::cpp`
/// only lowers a constructor here when it has a real, user-written body
/// (`examples/E04-classe-com-encapsulamento/NOTES.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Constructor {
    pub usr: String,
    /// Same ordinal `Expr::ConstructorCall::constructor_index` carries —
    /// computed once, the same way, by `lower::cpp::constructor_ordinal`, so
    /// a call site and the constructor it calls can never disagree about
    /// which one is primary. Not necessarily equal to this constructor's
    /// position in `Record::constructors`: that `Vec` is filled in
    /// definition-visitation order (`function_catalog::visit_cursor`), which
    /// for out-of-line members isn't guaranteed to match declaration order —
    /// `emit::dart` sorts by this field before deciding which constructor is
    /// unnamed.
    pub constructor_index: usize,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    /// `usr` of the corresponding `type_catalog::TypeDeclaration` — the join
    /// key between the type catalog and this IR, and what `Type::Record`
    /// points back to.
    pub usr: String,
    /// The chain of enclosing C++ namespaces, joined by `::` — empty when
    /// the record isn't namespaced. Captured for
    /// `function_catalog::apply_record_name_disambiguation` (two distinct
    /// records with the same bare `name` in different namespaces collide as
    /// the same Dart class otherwise — a real occurrence in the Verovio 6.2.0
    /// corpus, `docs/plans/diagnostico-verovio-6.2.0.md` achado 2): a
    /// namespace-qualified rename only kicks in for a record whose bare name
    /// actually collides with another's, so this field costs nothing for the
    /// overwhelming majority of records that never do.
    pub namespace: String,
    /// In declaration order — field order is part of the type's identity
    /// for `RecordConstruct`'s positional-constructor emission.
    pub fields: Vec<Field>,
    /// `static` data members — always emitted zero/default-initialized;
    /// `lower::cpp` doesn't read a static field's out-of-line initializer
    /// yet (`examples/E04-classe-com-encapsulamento/NOTES.md`), a
    /// deliberately narrow simplification, not a silent one.
    pub static_fields: Vec<Field>,
    pub constructors: Vec<Constructor>,
    pub methods: Vec<Method>,
    /// The single base class this record `extends`, if any (E06 —
    /// "herança simples") — populated only when the record has exactly one
    /// `CXXBaseSpecifier`. Two or more populates `mixins` instead, never
    /// both (see `lower::cpp::base_classes_of`).
    pub base_class: Option<BaseClass>,
    /// Every base class when the record has *more than one* (E09 —
    /// "herança múltipla"). `mapping::options_for`'s own multiple-
    /// inheritance decision always turns every base into a Dart mixin
    /// (`with A, B`, never `extends`) regardless of which specific option
    /// id it returns — so this list, not `base_class`, drives emission:
    /// `emit::dart::emit_record` prints `class X with A, B { ... }`, and a
    /// record *referenced* here (`Voador`/`Nadador`) is itself emitted as a
    /// Dart `mixin`, not `class` — Dart requires that: a class used via
    /// `with` can't have a non-default constructor, which the ordinary
    /// synthetic positional constructor every other record with fields
    /// gets (E03) would be, so the "am I used as a mixin" check
    /// (`emit::dart::emit_module`, gathered from every record's `mixins`
    /// across the whole `Module` before any single record is emitted)
    /// changes the field defaults and constructor emission for the
    /// referenced record.
    pub mixins: Vec<BaseClass>,
    /// A *real* destructor's body — `None` for no user-declared destructor,
    /// an empty/`= default` one (no teardown logic of its own, same
    /// distinction E06 already draws for deciding whether to lower a
    /// destructor at all — `examples/E06-heranca-simples/NOTES.md`), or one
    /// this IR doesn't otherwise represent. Never emitted as a Dart class
    /// member (Dart has no destructors) — `function_catalog::
    /// apply_raii_scope_guards` (E12) is the only consumer, splicing these
    /// statements into a `Stmt::TryFinally` at each local declaration of
    /// this record's type, with `Expr::This` substituted for that local's
    /// own name (`replace_this_with_ref`) so the body reads correctly
    /// outside the class it came from.
    pub destructor: Option<Vec<Stmt>>,
    pub origin: Origin,
}

/// A record's single base class — `usr`/`name` mirror `Type::Record`'s own
/// fields for the same reason: `emit::dart` needs the Dart-side name to
/// print `extends BaseName`, and doesn't have the whole `Module` in scope
/// everywhere a `Record` is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaseClass {
    pub usr: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub functions: Vec<Function>,
    pub records: Vec<Record>,
}
