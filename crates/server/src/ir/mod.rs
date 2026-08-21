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
    /// A C++ `enum`/`enum class` — `usr`/`name` mirror `Record`'s own
    /// fields, join key into `Module::enums`. Caso 4 of
    /// `docs/plans/verovio-6.2-pointer-types.md`.
    Enum {
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
    /// A contiguous buffer of unsigned bytes (`uint8_t*`/`mz_uint8*`) —
    /// emitted as Dart's `Uint8List`, rather than a generic `List<int>`, so
    /// the binary-data contract remains visible at the generated API
    /// boundary. Pointer uses become `Nullable(Bytes)` when the source
    /// pointer may be null.
    Bytes,
    /// `std::vector<T>` — the other half of E05's library adapter, mapped to
    /// Dart's `List<T>`. Only `T = Int` is exercised by any exemplo so far;
    /// the element type is still carried generally (not hardcoded to `Int`)
    /// so a future degrau that needs `vector<double>`/`vector<Record>`
    /// doesn't have to revisit this variant's shape.
    ///
    /// `std::list<T>` also lowers to this variant, not a distinct one:
    /// `docs/plans/verovio-6.2-pointer-types.md` caso 5 found Verovio's
    /// `ListOfObjects`/`ListOfConstObjects` (`typedef std::list<...>`)
    /// among the pointer catalog's "should be trivial" bucket, and the
    /// difference that actually matters for a raw `T*` pointee's mapping —
    /// whether the pointee is a type this IR can already represent at
    /// all — doesn't depend on `std::vector` vs `std::list`'s different
    /// (and, for this product, not yet emitted-into-Dart) performance
    /// characteristics. Both become Dart's `List<T>`, same gap E01's `int`
    /// overflow already accepted for a narrower but analogous reason: a
    /// working, analyzable program first, not a perf-preserving one.
    List(Box<Type>),
    /// `std::set<T>` — Dart's `Set<T>` is a direct, no-adapter-needed
    /// match (unlike `std::list`, no Dart core type shares `std::vector`'s
    /// shape closely enough to reuse `List`). Added alongside `Map` for
    /// caso 5 of `docs/plans/verovio-6.2-pointer-types.md`
    /// (`SetOfConstObjects` in Verovio 6.2.0).
    Set(Box<Type>),
    /// `std::map<K, V>` — Dart's `Map<K, V>`, same reasoning as `Set`.
    /// Caso 5 of `docs/plans/verovio-6.2-pointer-types.md`
    /// (`MapOfStrOptions` in Verovio 6.2.0).
    Map(Box<Type>, Box<Type>),
    /// `std::pair<A, B>` — a shared generated `SyntaxBridgePair<A, B>`.
    /// A named adapter (rather than `Tuple`) preserves C++'s stable `first`
    /// and `second` member names and remains the same nominal Dart type when
    /// it crosses generated source files.
    Pair(Box<Type>, Box<Type>),
    /// A C/C++ function pointer whose parameter and return types are all
    /// represented in this IR. Emitted as a typed Dart closure
    /// (`ReturnType Function(Args...)`); ABI callbacks that need native
    /// calling conventions remain a separate FFI bridge.
    Callback {
        return_type: Box<Type>,
        params: Vec<Type>,
    },
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
    Or,
    ShiftLeft,
    ShiftRight,
    BitAnd,
    BitXor,
    BitOr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaryOp {
    Neg,
    Not,
    PreIncrement,
    PreDecrement,
    PostIncrement,
    PostDecrement,
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
    NullLiteral {
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
    Conditional {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
        ty: Type,
        origin: Origin,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        ty: Type,
        origin: Origin,
    },
    /// A brace-enclosed initializer list (`{1, 2, 3}`) lowered to a Dart
    /// list literal — only when `ty` (`clang_getCursorType` on the
    /// `InitListExpr` cursor, already resolved by `lower::cpp`) is
    /// `Type::List`; every other destination (aggregate struct, `Set`,
    /// fixed C array) stays a bailout rather than guessing a Dart literal
    /// shape from an unverified type. `Type::Map` gets its own
    /// `Expr::MapLiteral` below instead, since a `std::map`'s brace
    /// initializer doesn't share this variant's flat "one value per
    /// element" shape. See `lower::cpp::lower_expr`'s
    /// `CXCursor_InitListExpr` branch.
    ListLiteral {
        items: Vec<Expr>,
        ty: Type,
        origin: Origin,
    },
    /// `std::map<K, V> m{ {k1, v1}, {k2, v2} };` (also `unordered_map`) —
    /// real Verovio trigger: static const lookup tables declared this way
    /// throughout the corpus (`midifunctor.cpp`, `iocmme.cpp`, ...). Unlike
    /// `ListLiteral`'s flat elements, a `std::map`'s initializer-list
    /// constructor wraps each entry in its own `std::pair<const K, V>`
    /// construction (2 args, key then value) — `lower::cpp` extracts those
    /// two arguments directly rather than routing through a general
    /// `std::pair` construction path this module doesn't otherwise need.
    /// `ty` is always `Type::Map(K, V)`, matching the destination's own
    /// resolved type — this only ever gets built from a construct call
    /// libclang already resolved to that owner, never guessed.
    MapLiteral {
        entries: Vec<(Expr, Expr)>,
        ty: Type,
        origin: Origin,
    },
    /// Dart's `operand is T` runtime type check — the check's own value is
    /// always `bool`. Currently produced only by `dynamic_cast<T*>(operand)`'s
    /// translation (see `lower::cpp::lower_dynamic_cast_expr`, which wraps
    /// this as the condition of an `Expr::Conditional`: `operand is T ?
    /// operand : null` — Dart's flow-sensitive type promotion inside a
    /// ternary's condition→then branch means the `then` branch needs no
    /// separate cast), never emitted from any other C++ construct.
    Is {
        operand: Box<Expr>,
        target_type: Type,
        origin: Origin,
    },
    /// Dart's checked `operand as T` cast — produced by a C++
    /// `static_cast`/C-style cast that narrows a pointer down a class
    /// hierarchy (`Base*` → `Derived*`, F7 —
    /// `docs/prompts/2026-08-21-05-downcast-de-hierarquia-preservado.md`).
    /// Unlike `Is`'s ternary (built only for the *checked* `dynamic_cast`),
    /// `static_cast` is unchecked in C++ itself: an incorrect cast is
    /// undefined behavior, not a null result, so translating it to `is T ?
    /// x : null` would silently turn a real program bug into a quiet
    /// `null` — exactly the "silêncio é proibido" failure `AGENTS.md`
    /// forbids. `as T?` throws a `TypeError` instead, the honest
    /// equivalent. `ty` is always the cast's target `Type::Nullable(Record)`.
    /// Needs no simple-operand guard the way `Is`/`dynamic_cast` does:
    /// `as` evaluates `operand` exactly once, so it covers call/field-access
    /// operands `dynamic_cast`'s ternary form has to bail out on.
    As {
        operand: Box<Expr>,
        ty: Type,
        origin: Origin,
    },
    /// C++ assignment used as an *expression*, not a whole statement
    /// (`while ((x = foo()) != nullptr)`, or the same shape reached
    /// indirectly when an intervening `libclang` wrapper cursor — e.g. a
    /// template-instantiated call's cleanup node — keeps a plain-looking
    /// `x = foo();` statement from being recognized as
    /// `CXCursor_BinaryOperator` at the statement level, confirmed as the
    /// real Verovio trigger in `adjustarticfunctor.cpp`'s `yIn =
    /// std::max(yAboveStem, -staffHeight);`). Dart's own `=` is a real
    /// expression too, evaluating to the assigned value — confirmed with a
    /// real `dart analyze`/`dart run` before this was assumed, not
    /// guessed — so this maps 1:1 onto Dart's assignment expression rather
    /// than needing a hoisted temporary statement. Always parenthesized on
    /// emission (`(target = value)`): Dart's `=` has the same low
    /// precedence C++'s does, so an unparenthesized `x = y != null` would
    /// parse as `x = (y != null)`, not `(x = y) != null`. Scoped to the
    /// same two simple target shapes `Stmt::Assign`/`Stmt::FieldAssign`
    /// already support (a bare local/field) — anything else stays an
    /// honest bailout, the same restriction `lower_assign_stmt` already
    /// enforces for the statement form.
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
        ty: Type,
        origin: Origin,
    },
    /// An implicit scalar conversion C++ inserted on `operand` — currently
    /// `int` → `double` and C++ truthiness `int` → `bool`. `lower::cpp` must
    /// keep it explicit rather than discarding it as sugar: Dart neither
    /// widens an integer expression to `double` nor accepts one as a boolean
    /// condition. `emit::dart` turns these into `.toDouble()` and `!= 0`.
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
    /// A non-const C++ map subscript. Reading it inserts a typed default
    /// value on miss; writing it is an ordinary map assignment. The emitter
    /// needs this distinct from Index so neither behavior is silently lost.
    MapIndexOrInsert {
        target: Box<Expr>,
        index: Box<Expr>,
        default_value: Box<Expr>,
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
    /// `std::basic_string::find(needle)` — searches the UTF-8 bytes rather
    /// than Dart `String` UTF-16 code units, retaining C++'s offset domain
    /// and `-1` not-found sentinel through `List<int>.indexOf`.
    StringByteIndexOf {
        target: Box<Expr>,
        needle: Box<Expr>,
        origin: Origin,
    },
    StringByteAt {
        target: Box<Expr>,
        index: Box<Expr>,
        ty: Type,
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
    /// An unsupported expression whose C++ result type was still available
    /// from libclang. Unlike the legacy `Unsupported` shape, this preserves
    /// the static type expected by the surrounding Dart expression so the
    /// throwing bailout can remain type-safe without `dynamic`.
    UnsupportedTyped {
        reason: String,
        ty: Type,
        origin: Origin,
    },
}

impl Expr {
    pub fn origin(&self) -> &Origin {
        match self {
            Self::IntLiteral { origin, .. }
            | Self::DoubleLiteral { origin, .. }
            | Self::BoolLiteral { origin, .. }
            | Self::NullLiteral { origin }
            | Self::StringLiteral { origin, .. }
            | Self::Ref { origin, .. }
            | Self::Binary { origin, .. }
            | Self::Conditional { origin, .. }
            | Self::Unary { origin, .. }
            | Self::ListLiteral { origin, .. }
            | Self::MapLiteral { origin, .. }
            | Self::Is { origin, .. }
            | Self::As { origin, .. }
            | Self::Assign { origin, .. }
            | Self::Convert { origin, .. }
            | Self::Call { origin, .. }
            | Self::FieldAccess { origin, .. }
            | Self::RecordConstruct { origin, .. }
            | Self::ConstructorCall { origin, .. }
            | Self::This { origin, .. }
            | Self::Index { origin, .. }
            | Self::MapIndexOrInsert { origin, .. }
            | Self::StringByteLength { origin, .. }
            | Self::StringByteIndexOf { origin, .. }
            | Self::StringByteAt { origin, .. }
            | Self::Tuple { origin, .. }
            | Self::Unsupported { origin, .. }
            | Self::UnsupportedTyped { origin, .. } => origin,
        }
    }

    /// Whether this expression is a shape Dart actually allows on the left
    /// of `=` — a bare reference, a field access, or an index write. Used
    /// to guard the "unwrap `Expr::Convert` to reach the real out-param
    /// dereference target" shortcut (`emit::dart`'s `Stmt::ExprAssign`
    /// handling): `Expr::Convert`'s own operand is usually one of these
    /// three, but since `Expr::Assign` (assignment used as an expression)
    /// was added, a doubly-nested case became representable where it
    /// wasn't before — `*(field = new T()) = value;`, whose dereference's
    /// operand is itself `field = new T()`, an `Expr::Assign` that is a
    /// valid Dart *value* but never a valid Dart assignment *target*
    /// (`(x = y) = z;` doesn't compile). Without this guard, that shape
    /// would unwrap into exactly that invalid Dart.
    pub fn is_assignable_lvalue(&self) -> bool {
        matches!(
            self,
            Self::Ref { .. } | Self::FieldAccess { .. } | Self::Index { .. }
        )
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
    /// Assignment through any expression Dart accepts on the left hand side,
    /// such as an index, or a receiver extracted from an overloaded C++
    /// assignment operator. `Assign`/`FieldAssign` remain compact forms for
    /// their common simple cases.
    ExprAssign {
        target: Expr,
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
    /// `do { body } while (condition);` — unlike a `While`, its body runs at
    /// least once. Dart has the same control-flow construct, so keeping it
    /// explicit avoids a lossy desugaring.
    DoWhile {
        body: Vec<Stmt>,
        condition: Expr,
        origin: Origin,
    },
    For {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        increment: Option<Box<Stmt>>,
        body: Vec<Stmt>,
        origin: Origin,
    },
    /// `for (T item : iterable)` — a value or const-reference range binding.
    /// A mutable C++ reference remains unsupported until the collection
    /// adapter can represent writes through it without losing aliasing.
    ForEach {
        name: String,
        ty: Type,
        is_final: bool,
        write_back: bool,
        iterable: Expr,
        body: Vec<Stmt>,
        origin: Origin,
    },
    Break {
        origin: Origin,
    },
    Continue {
        origin: Origin,
    },
    /// `continue <label>;` — Dart's own explicit-fallthrough syntax for a
    /// `switch` `case` that falls into the next one without a `break`
    /// (`docs/plans/bailouts-verovio-6.2.0.md`'s "a case falls through..."
    /// family). Distinct from `Continue` (a loop's own jump) — this always
    /// targets a label on a sibling `SwitchCase`/`default`
    /// (`SwitchCase::label`), never a loop.
    ContinueLabel {
        label: String,
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
    /// `switch (scrutinee) { case v: body... case w: ... default: ... }`.
    /// Each `SwitchCase` already carries every stacked label it shares a
    /// body with (`case 2: case 3: baz(); break;` → one `SwitchCase` with
    /// `values: [2, 3]`) — `lower::cpp::lower_switch_stmt`'s own doc comment
    /// has the empirically-confirmed reason `libclang` needs that unwrapping
    /// (a `CaseStmt`'s single child is only ever the *next* statement, which
    /// is itself another `CaseStmt` when labels stack, not a flat list).
    /// `default` is `None` when the C++ switch has no `default:` label —
    /// distinct from `Some(vec![])`, an empty-but-present default.
    ///
    /// Every case's `body` must already end in a jump (`Break`/`Continue`/
    /// `Return`/`Throw`) or be empty — Dart, unlike C++, rejects implicit
    /// fallthrough out of a non-empty case as a compile error, so
    /// `lower_switch_stmt` never constructs one that doesn't; a switch
    /// containing genuine C++ fallthrough stays a `Stmt::Unsupported`
    /// instead.
    Switch {
        scrutinee: Expr,
        cases: Vec<SwitchCase>,
        default: Option<Vec<Stmt>>,
        origin: Origin,
    },
    Unsupported {
        reason: String,
        origin: Origin,
    },
}

/// One `case`/stacked-`case`-group of a [`Stmt::Switch`] — see its own doc
/// comment for `values`/`body`'s exact shape and the fallthrough rule both
/// must already satisfy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwitchCase {
    pub values: Vec<Expr>,
    pub body: Vec<Stmt>,
    /// A label a preceding, falling-through case's `Stmt::ContinueLabel`
    /// jumps to (`docs/plans/bailouts-verovio-6.2.0.md`'s "a case falls
    /// through..." family). `None` when nothing falls into this case.
    pub label: Option<String>,
}

impl Stmt {
    pub fn origin(&self) -> &Origin {
        match self {
            Self::Return { origin, .. }
            | Self::VarDecl { origin, .. }
            | Self::Assign { origin, .. }
            | Self::FieldAssign { origin, .. }
            | Self::ExprAssign { origin, .. }
            | Self::If { origin, .. }
            | Self::While { origin, .. }
            | Self::DoWhile { origin, .. }
            | Self::For { origin, .. }
            | Self::ForEach { origin, .. }
            | Self::Break { origin }
            | Self::Continue { origin }
            | Self::ContinueLabel { origin, .. }
            | Self::ExprStmt { origin, .. }
            | Self::Throw { origin, .. }
            | Self::TryCatch { origin, .. }
            | Self::TryFinally { origin, .. }
            | Self::TupleAssign { origin, .. }
            | Self::Switch { origin, .. }
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
    /// Already the *Dart* name — a `private` C++ member (E04's visibility
    /// requirement) gets its leading `_` baked in here, at the same place
    /// every reference to the field resolves its name from
    /// (`lower::cpp::dart_member_name`), so a field's declaration and every
    /// access of it can never disagree on whether it's private. `protected`
    /// is *not* treated as private — see `dart_member_name`'s doc comment.
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

/// One `enum`/`enum class` declaration — `usr`/`name` are the join key into
/// `Type::Enum`, the same relationship `Record`/`Type::Record` already have.
/// Caso 4 of `docs/plans/verovio-6.2-pointer-types.md`: a C++ enum has the
/// same statically-finite-set-of-values guarantee a `struct`/`class` has
/// (`mapping::pointer_options_for`'s case A10 reasoning applies unchanged),
/// but nothing in this IR could represent an enum *at all* before this —
/// not as a value, not as a pointee — so `lower::cpp::lower_type` always
/// fell through to `Type::Unsupported` for one, regardless of whether a
/// pointer was involved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Enum {
    pub name: String,
    pub usr: String,
    /// Enumerator names, source order. `enum class Foo { A, B };` and
    /// unscoped `enum Foo { A, B };` both lower here the same way — Dart's
    /// `enum Foo { a, b }` has no unscoped/scoped distinction to preserve,
    /// and every enumerator this IR ever sees is already schema-qualified
    /// by `qualified_static_member_name` at the reference site
    /// (`EnumName.enumerator`), not by how it was declared.
    pub variants: Vec<String>,
    /// Each enumerator's real C++ value (`clang_getEnumConstantDeclValue`),
    /// same index alignment as `variants`. C++ enumerators are not
    /// guaranteed to be 0-based/sequential/gapless — Verovio itself declares
    /// enums that are neither — so Dart's own `.index` (always the
    /// declaration position) is only an accident away from being a
    /// different number than the C++ program actually computes. `emit::dart`
    /// gives every Dart enum an explicit `value` field carrying this number,
    /// so a C++-to-`int` conversion (`Expr::Convert` to `Type::Int` with an
    /// `Enum`-typed operand) reads `.value`, never `.index`.
    pub values: Vec<i64>,
    pub origin: Origin,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub functions: Vec<Function>,
    pub records: Vec<Record>,
    pub enums: Vec<Enum>,
}
