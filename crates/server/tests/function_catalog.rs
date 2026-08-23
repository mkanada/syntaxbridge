//! Exercises the real `libclang` function-catalog extraction (US-5) end to
//! end, mirroring `crates/server/tests/type_catalog.rs`'s style: a small
//! fixture written directly to a temp workspace, parsed through
//! `function_catalog::extract_function_catalog` without going through the
//! whole `project_service::create_project` pipeline.
//!
//! Needs a real `libclang` loadable in the environment (see
//! `crates/server/src/type_catalog.rs`'s module docs) — run inside the
//! Flatpak sandbox via `scripts/test-in-flatpak.sh` for that to be
//! guaranteed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use syntax_bridge_server::function_catalog::{self, CallResolution, FunctionDeclarationKind};
use syntax_bridge_server::ingest::CompilationUnit;
use syntax_bridge_server::ir;

/// Deliberately contains, per US-5's testability conditions: a hierarchy with
/// a virtual method redefined (`Circle::area` overrides `Shape::area`) and
/// another not redefined (`Shape::perimeter`, inherited by `Circle` as-is), a
/// pair of overloads (`add(int,int)` / `add(double,double)`), a pointer to
/// function (`BinaryOp`, called indirectly inside `apply`), and a
/// function-like macro (`SQUARE`).
const FUNCTIONS_CPP: &str = r#"
#define SQUARE(x) ((x) * (x))

class Shape {
public:
    virtual double area() const { return 0.0; }
    virtual double perimeter() const { return 0.0; }
    virtual ~Shape() {}
};

class Circle : public Shape {
public:
    explicit Circle(double r) : radius(r) {}
    double area() const override { return 3.14159 * radius * radius; }

private:
    double radius;
};

int add(int a, int b) {
    return a + b;
}

double add(double a, double b) {
    return a + b;
}

typedef int (*BinaryOp)(int, int);

int apply(BinaryOp op, int x, int y) {
    return op(x, y);
}

double describe(const Shape& shape) {
    return shape.area();
}

int compute() {
    int direct = add(1, 2);
    int squared = SQUARE(3);
    int via_pointer = apply(add, 4, 5);
    Circle circle(2.0);
    double area = describe(circle);
    return direct + squared + via_pointer + (int)area;
}
"#;

fn write_fixture(project_root: &Path) -> CompilationUnit {
    fs::create_dir_all(project_root).expect("create project dir");
    let file_path = project_root.join("functions.cpp");
    fs::write(&file_path, FUNCTIONS_CPP).expect("write functions.cpp");

    CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    }
}

/// Criterion 1: every free function, method, constructor, destructor and
/// macro is cataloged with a full signature; criterion 2: overloads are
/// distinct entries.
#[test]
fn extract_function_catalog_lists_every_callable_with_full_signature() {
    let workspace =
        TempWorkspace::new("function-catalog-declarations").expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    let unit = write_fixture(&project_root);

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let by_name = |name: &str| -> Vec<_> {
        catalog
            .declarations
            .iter()
            .filter(|declaration| declaration.name == name)
            .collect()
    };

    let add_overloads = by_name("add");
    assert_eq!(
        add_overloads.len(),
        2,
        "expected two distinct `add` overloads: {add_overloads:#?}"
    );
    assert_ne!(
        add_overloads[0].usr, add_overloads[1].usr,
        "overloads must have distinct usr"
    );
    assert_ne!(
        add_overloads[0].signature, add_overloads[1].signature,
        "overloads must have distinct signatures"
    );
    assert!(
        add_overloads
            .iter()
            .all(|declaration| declaration.kind == FunctionDeclarationKind::FreeFunction)
    );

    let area_methods = by_name("area");
    assert_eq!(
        area_methods.len(),
        2,
        "expected Shape::area and Circle::area as distinct entries: {area_methods:#?}"
    );
    assert!(area_methods.iter().all(|declaration| declaration.kind
        == FunctionDeclarationKind::Method
        && declaration.is_virtual));

    // Criterion 4: `perimeter` is not redefined by `Circle`, so it's
    // cataloged exactly once, attributed to `Shape`.
    let perimeter_methods = by_name("perimeter");
    assert_eq!(
        perimeter_methods.len(),
        1,
        "expected perimeter to be attributed only to Shape, not duplicated for Circle: \
         {perimeter_methods:#?}"
    );
    assert!(perimeter_methods[0].file.ends_with("functions.cpp"));

    let constructors: Vec<_> = catalog
        .declarations
        .iter()
        .filter(|declaration| declaration.kind == FunctionDeclarationKind::Constructor)
        .collect();
    assert_eq!(
        constructors.len(),
        1,
        "expected Circle's constructor: {constructors:#?}"
    );

    let destructors: Vec<_> = catalog
        .declarations
        .iter()
        .filter(|declaration| declaration.kind == FunctionDeclarationKind::Destructor)
        .collect();
    assert_eq!(
        destructors.len(),
        1,
        "expected Shape's virtual destructor: {destructors:#?}"
    );
    assert!(destructors[0].is_virtual);

    let square_macro = catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "SQUARE")
        .unwrap_or_else(|| {
            panic!(
                "expected SQUARE macro in catalog: {:#?}",
                catalog.declarations
            )
        });
    assert_eq!(square_macro.kind, FunctionDeclarationKind::FunctionMacro);

    for kind in [
        FunctionDeclarationKind::FreeFunction,
        FunctionDeclarationKind::Method,
    ] {
        assert!(
            catalog
                .declarations
                .iter()
                .filter(|declaration| declaration.kind == kind)
                .all(|declaration| !declaration.signature.is_empty()),
            "expected every {kind:?} to carry a non-empty signature"
        );
    }
}

/// A worker's `parse_chunk` reuses one `ir_records: Vec<ir::Record>` across
/// every compilation unit it processes (see
/// `docs/plans/verovio-6.2-pointer-types.md`'s duplicate-definition
/// investigation): a class fully defined in a header, reached from more than
/// one translation unit in the same worker's chunk, must still end up with
/// exactly one `ir::Record` carrying exactly one copy of each method — not
/// one record per translation unit that included the header, nor a method
/// list re-appended once per inclusion. `unit_count` is derived from
/// `available_parallelism` (`2 * workers + 1`) specifically so this
/// reproduces regardless of how many cores the test happens to run on: it
/// guarantees `chunk_size >= 3` (`div_ceil((2p+1), p) == 3` for any `p >=
/// 1`), so at least one worker is guaranteed to parse more than one
/// translation unit that includes the shared header.
#[test]
fn extract_function_catalog_does_not_duplicate_a_records_methods_across_a_workers_chunk() {
    let workspace =
        TempWorkspace::new("function-catalog-chunk-dedup").expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");

    fs::write(
        project_root.join("shared.h"),
        r#"
#pragma once
class Plain {
public:
    int GetValue() const { return m_value; }
    void SetValue(int v) { m_value = v; }
private:
    int m_value = 0;
};
"#,
    )
    .expect("write shared.h");

    let worker_count = std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1);
    let unit_count = 2 * worker_count + 1;

    let units: Vec<CompilationUnit> = (0..unit_count)
        .map(|index| {
            let file_name = format!("unit{index}.cpp");
            fs::write(project_root.join(&file_name), "#include \"shared.h\"\n")
                .expect("write translation unit");
            CompilationUnit {
                directory: project_root.display().to_string(),
                file: project_root.join(&file_name).display().to_string(),
                command: None,
                arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
            }
        })
        .collect();

    let catalog = function_catalog::extract_function_catalog(&units, &project_root, None)
        .expect("extract function catalog");

    let plain_records: Vec<_> = catalog
        .ir_records
        .iter()
        .filter(|record| record.name == "Plain")
        .collect();
    assert_eq!(
        plain_records.len(),
        1,
        "expected exactly one Plain record across all workers: {plain_records:#?}"
    );

    let method_names: Vec<&str> = plain_records[0]
        .methods
        .iter()
        .map(|method| method.name.as_str())
        .collect();
    assert_eq!(
        method_names,
        vec!["GetValue", "SetValue"],
        "expected each method exactly once, not once per translation unit in a worker's chunk: \
         {method_names:?}"
    );
}

/// Regression test for achado 1 of `docs/plans/diagnostico-verovio-6.2.0.md`
/// (≈85% of every `dart analyze` error on the real Verovio 6.2.0 corpus):
/// a const/non-const getter pair with *no parameters on either side*
/// (`GetOffsetInterface()`/`GetOffsetInterface() const`, taken verbatim from
/// that diagnosis) is exactly the shape `mapping::overload_options_for`
/// already recognizes as `"renomear-const-nao-const"` — but
/// `function_catalog::apply_overload_renames` used to hand both sides to
/// `dart_overload_name`, which only ever appends a *parameter-type* suffix.
/// With no parameters on either side, that suffix is empty for both, so
/// "renaming" produced the exact same name twice — the two `ir::Method`
/// entries `emit::dart` then prints as two identical-looking declarations in
/// the same class, `dart analyze`'s `duplicate_definition`.
#[test]
fn a_const_and_non_const_overload_with_no_parameters_get_distinct_dart_names() {
    let workspace =
        TempWorkspace::new("function-catalog-const-overload").expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");
    fs::write(
        project_root.join("accid.cpp"),
        r#"
class Accid {
public:
    int GetOffsetInterface() { return 0; }
    int GetOffsetInterface() const { return 1; }
};
"#,
    )
    .expect("write accid.cpp");

    let unit = CompilationUnit {
        directory: project_root.display().to_string(),
        file: project_root.join("accid.cpp").display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let accid = catalog
        .ir_records
        .iter()
        .find(|record| record.name == "Accid")
        .unwrap_or_else(|| panic!("expected an Accid record: {:#?}", catalog.ir_records));
    let method_names: Vec<&str> = accid
        .methods
        .iter()
        .map(|method| method.name.as_str())
        .collect();
    assert_eq!(
        method_names.len(),
        2,
        "expected both overloads to survive lowering: {method_names:?}"
    );
    assert_ne!(
        method_names[0], method_names[1],
        "the const and non-const overloads must not collide under the same Dart name: \
         {method_names:?}"
    );
}

/// Achado 1 restante (`docs/plans/diagnostico-verovio-6.2.0.md`): Verovio's
/// real `Accid::IsAlignedWithSameLayer` shape — a getter and a setter
/// sharing a name, differing both in arity (0 vs. 1 parameter) *and* return
/// type (`bool` vs. `void`). Before this fix, `mapping::overload_options_for`
/// read differing arity alone as `"parametro-opcional"` (fold into one Dart
/// member with an optional parameter, left unrenamed by
/// `apply_overload_renames`) — the two methods survived lowering under the
/// same Dart name, `duplicate_definition`. Must end up renamed, same as any
/// other overload Dart can't dispatch by type.
#[test]
fn a_getter_and_setter_pair_differing_in_both_arity_and_return_type_get_distinct_dart_names() {
    let workspace = TempWorkspace::new("function-catalog-arity-return-overload")
        .expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");
    fs::write(
        project_root.join("accid.cpp"),
        r#"
class Accid {
public:
    bool IsAlignedWithSameLayer() const { return aligned; }
    void IsAlignedWithSameLayer(bool value) { aligned = value; }
private:
    bool aligned = false;
};
"#,
    )
    .expect("write accid.cpp");

    let unit = CompilationUnit {
        directory: project_root.display().to_string(),
        file: project_root.join("accid.cpp").display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let accid = catalog
        .ir_records
        .iter()
        .find(|record| record.name == "Accid")
        .unwrap_or_else(|| panic!("expected an Accid record: {:#?}", catalog.ir_records));
    let method_names: Vec<&str> = accid
        .methods
        .iter()
        .map(|method| method.name.as_str())
        .collect();
    assert_eq!(
        method_names.len(),
        2,
        "expected both the getter and setter to survive lowering: {method_names:?}"
    );
    assert_ne!(
        method_names[0], method_names[1],
        "the getter and setter must not collide under the same Dart name: {method_names:?}"
    );
}

/// Achado 1 restante, second sub-case (`docs/plans/diagnostico-verovio-6.2.0.md`):
/// found in the real Verovio 6.2.0 corpus after the getter/setter fix above
/// (e.g. `CalculateDotLocations`, `GetCrossStaffExtremes`) — two overloads
/// whose only distinguishing parameter is itself `Type::Unsupported` (here,
/// a raw pointer to a scalar, case C01 — never bridged) with two genuinely
/// *different* C++ spellings (`int*` vs. `double*`). `renomear-por-tipo`
/// correctly decides these need distinct names, but
/// `lower::cpp::overload_type_suffix` mapped every `Unsupported` parameter
/// to the same fixed suffix (`"Unsupported"`), so `dart_overload_name`
/// computed the identical renamed name for both — the same
/// `duplicate_definition` achado 1 exists to prevent, just one level
/// deeper: past the renaming decision, into the suffix computation itself.
#[test]
fn two_overloads_distinguished_only_by_different_unsupported_parameter_types_get_distinct_dart_names()
 {
    let workspace = TempWorkspace::new("function-catalog-unsupported-overload")
        .expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");
    fs::write(
        project_root.join("ponte.cpp"),
        r#"
void Escrever(int* valor) {}
void Escrever(double* valor) {}
"#,
    )
    .expect("write ponte.cpp");

    let unit = CompilationUnit {
        directory: project_root.display().to_string(),
        file: project_root.join("ponte.cpp").display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let function_names: Vec<&str> = catalog
        .ir_functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(
        function_names.len(),
        2,
        "expected both overloads to survive lowering: {function_names:?}"
    );
    assert_ne!(
        function_names[0], function_names[1],
        "overloads distinguished only by different Unsupported parameter types must not \
         collide under the same Dart name: {function_names:?}"
    );
}

/// F13/tarefa 12, test 1
/// (`docs/prompts/2026-08-21-12-overloads-const-e-colisoes-de-nome.md`): a
/// const/non-const pair with bodies, called from two call sites whose own
/// `const`-ness picks a different overload (a `Box&` receiver resolves to
/// the non-const `GetX`, a `const Box&` receiver to the `const` one) — not
/// just that the two declarations end up with distinct Dart names, but that
/// each call site's `callee_usr`/`callee_name` still points at the *correct*
/// one after the rename (the failure mode the prompt calls out explicitly:
/// reaching the declaration but not every call site).
#[test]
fn a_const_and_non_const_overload_route_calls_to_the_matching_renamed_method() {
    let workspace = TempWorkspace::new("function-catalog-const-overload-calls")
        .expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");
    fs::write(
        project_root.join("box.cpp"),
        r#"
class Box {
public:
    int GetX() { return 1; }
    int GetX() const { return 2; }
};

int ReadMutable(Box& box) { return box.GetX(); }
int ReadConst(const Box& box) { return box.GetX(); }
"#,
    )
    .expect("write box.cpp");

    let unit = CompilationUnit {
        directory: project_root.display().to_string(),
        file: project_root.join("box.cpp").display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let box_record = catalog
        .ir_records
        .iter()
        .find(|record| record.name == "Box")
        .unwrap_or_else(|| panic!("expected a Box record: {:#?}", catalog.ir_records));
    assert_eq!(box_record.methods.len(), 2, "{:#?}", box_record.methods);
    assert_ne!(
        box_record.methods[0].name, box_record.methods[1].name,
        "the const and non-const GetX overloads must not collide: {:?}",
        box_record.methods
    );

    let get_x_decls: Vec<_> = catalog
        .declarations
        .iter()
        .filter(|declaration| declaration.name == "GetX")
        .collect();
    assert_eq!(get_x_decls.len(), 2, "{get_x_decls:#?}");
    let const_decl = get_x_decls
        .iter()
        .find(|declaration| declaration.signature.contains("const"))
        .expect("a const GetX overload");
    let non_const_decl = get_x_decls
        .iter()
        .find(|declaration| !declaration.signature.contains("const"))
        .expect("a non-const GetX overload");

    let name_for_usr = |usr: &str| -> String {
        box_record
            .methods
            .iter()
            .find(|method| method.usr == usr)
            .unwrap_or_else(|| panic!("no Box method for usr {usr}: {:#?}", box_record.methods))
            .name
            .clone()
    };
    let const_name = name_for_usr(&const_decl.usr);
    let non_const_name = name_for_usr(&non_const_decl.usr);

    fn call_target(body: &[ir::Stmt]) -> (String, String) {
        for stmt in body {
            if let ir::Stmt::Return {
                value:
                    Some(ir::Expr::Call {
                        callee_usr,
                        callee_name,
                        ..
                    }),
                ..
            } = stmt
            {
                return (callee_usr.clone(), callee_name.clone());
            }
        }
        panic!("expected a Call expr in a Return statement: {body:#?}");
    }

    let read_mutable = catalog
        .ir_functions
        .iter()
        .find(|function| function.name == "ReadMutable")
        .expect("ReadMutable");
    let read_const = catalog
        .ir_functions
        .iter()
        .find(|function| function.name == "ReadConst")
        .expect("ReadConst");

    let (mutable_callee_usr, mutable_callee_name) = call_target(&read_mutable.body);
    let (const_callee_usr, const_callee_name) = call_target(&read_const.body);

    assert_eq!(mutable_callee_usr, non_const_decl.usr);
    assert_eq!(mutable_callee_name, non_const_name);
    assert_eq!(const_callee_usr, const_decl.usr);
    assert_eq!(const_callee_name, const_name);
}

/// F13/tarefa 12, test 2: two free-function `operator<<` overloads in the
/// same file (Verovio's real shape — `lower::cpp::dart_operator_bridge_name`
/// maps every `operator<<` to the same `"streamInsert"` bridge name,
/// regardless of which class it inserts) must bridge to *distinct* Dart
/// names, and each call site (`a << 2`, overload-resolved by `a`'s static
/// type) must still call the right one.
#[test]
fn two_free_operator_overloads_bridge_to_distinct_names_and_calls_resolve() {
    let workspace = TempWorkspace::new("function-catalog-operator-bridge-overload")
        .expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");
    fs::write(
        project_root.join("shift.cpp"),
        r#"
class Foo {
public:
    int value = 0;
};

class Bar {
public:
    int amount = 0;
};

Foo operator<<(const Foo& a, int shift) {
    Foo result;
    result.value = a.value << shift;
    return result;
}

Bar operator<<(const Bar& a, int shift) {
    Bar result;
    result.amount = a.amount << shift;
    return result;
}

Foo ShiftFoo(const Foo& a) {
    return a << 2;
}

Bar ShiftBar(const Bar& a) {
    return a << 2;
}
"#,
    )
    .expect("write shift.cpp");

    let unit = CompilationUnit {
        directory: project_root.display().to_string(),
        file: project_root.join("shift.cpp").display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let operator_decls: Vec<_> = catalog
        .declarations
        .iter()
        .filter(|declaration| declaration.name == "operator<<")
        .collect();
    assert_eq!(operator_decls.len(), 2, "{operator_decls:#?}");

    let name_for_usr = |usr: &str| -> String {
        catalog
            .ir_functions
            .iter()
            .find(|function| function.usr == usr)
            .unwrap_or_else(|| panic!("no ir function for usr {usr}: {:#?}", catalog.ir_functions))
            .name
            .clone()
    };

    let foo_decl = operator_decls
        .iter()
        .find(|declaration| declaration.signature.contains("Foo"))
        .expect("the Foo overload");
    let bar_decl = operator_decls
        .iter()
        .find(|declaration| declaration.signature.contains("Bar"))
        .expect("the Bar overload");
    let foo_name = name_for_usr(&foo_decl.usr);
    let bar_name = name_for_usr(&bar_decl.usr);
    assert_ne!(
        foo_name, bar_name,
        "the two operator<< bridge names must not collide"
    );
    for name in [&foo_name, &bar_name] {
        assert!(
            !name.starts_with("operator"),
            "expected a bridged Dart-safe name, got {name:?}"
        );
    }

    fn call_target(body: &[ir::Stmt]) -> (String, String) {
        for stmt in body {
            if let ir::Stmt::Return {
                value:
                    Some(ir::Expr::Call {
                        callee_usr,
                        callee_name,
                        ..
                    }),
                ..
            } = stmt
            {
                return (callee_usr.clone(), callee_name.clone());
            }
        }
        panic!("expected a Call expr in a Return statement: {body:#?}");
    }

    let shift_foo = catalog
        .ir_functions
        .iter()
        .find(|function| function.name == "ShiftFoo")
        .expect("ShiftFoo");
    let shift_bar = catalog
        .ir_functions
        .iter()
        .find(|function| function.name == "ShiftBar")
        .expect("ShiftBar");

    let (foo_callee_usr, foo_callee_name) = call_target(&shift_foo.body);
    let (bar_callee_usr, bar_callee_name) = call_target(&shift_bar.body);

    assert_eq!(foo_callee_usr, foo_decl.usr);
    assert_eq!(foo_callee_name, foo_name);
    assert_eq!(bar_callee_usr, bar_decl.usr);
    assert_eq!(bar_callee_name, bar_name);
}

/// F13/tarefa 12, test 3 (residual case): two overloads whose parameter
/// types are only distinguished in C++ (`const char*` vs. `char*`) but map
/// to the *identical* Dart type (`String?`) — `dart_overload_name`'s suffix
/// can't tell them apart, so this is the family's own last-resort ordinal
/// fallback, not the type-suffix mechanism the other tests exercise.
#[test]
fn two_overloads_that_map_to_the_identical_dart_signature_still_get_distinct_names() {
    let workspace = TempWorkspace::new("function-catalog-residual-overload")
        .expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");
    fs::write(
        project_root.join("ponte.cpp"),
        r#"
void Escrever(const char* valor) {}
void Escrever(char* valor) {}
"#,
    )
    .expect("write ponte.cpp");

    let unit = CompilationUnit {
        directory: project_root.display().to_string(),
        file: project_root.join("ponte.cpp").display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let function_names: Vec<&str> = catalog
        .ir_functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(
        function_names.len(),
        2,
        "expected both overloads to survive lowering: {function_names:?}"
    );
    assert_ne!(
        function_names[0], function_names[1],
        "overloads that map to the identical Dart parameter type must still get distinct \
         names: {function_names:?}"
    );
}

/// F13/tarefa 12, test 4: Verovio's real `HumTool::run`/`Options::setValue`
/// shape — overloads differing in *both* arity and, within a shared arity,
/// parameter type, every one returning the same type. Before this fix,
/// `mapping::overload_options_for` read "arities differ, return types agree"
/// alone as `"parametro-opcional"` (fold into one Dart member with an
/// optional parameter) without checking that each arity actually has a
/// single signature to fold — a fold `apply_overload_renames` never performs
/// anyway, so the whole group survived lowering under its shared original
/// name, `duplicate_definition`. All three must end up distinctly named.
#[test]
fn overloads_mixing_arity_and_same_arity_type_differences_all_get_distinct_names() {
    let workspace = TempWorkspace::new("function-catalog-mixed-arity-type-overload")
        .expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");
    fs::write(
        project_root.join("tool.cpp"),
        r#"
class Tool {
public:
    bool Run(int input) { return input > 0; }
    bool Run(int input, int extra) { return input > extra; }
    bool Run(double input) { return input > 0.0; }
};
"#,
    )
    .expect("write tool.cpp");

    let unit = CompilationUnit {
        directory: project_root.display().to_string(),
        file: project_root.join("tool.cpp").display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let tool = catalog
        .ir_records
        .iter()
        .find(|record| record.name == "Tool")
        .unwrap_or_else(|| panic!("expected a Tool record: {:#?}", catalog.ir_records));
    let method_names: Vec<&str> = tool
        .methods
        .iter()
        .map(|method| method.name.as_str())
        .collect();
    assert_eq!(
        method_names.len(),
        3,
        "expected all three Run overloads to survive lowering: {method_names:?}"
    );
    let mut unique_names: Vec<&str> = method_names.clone();
    unique_names.sort_unstable();
    unique_names.dedup();
    assert_eq!(
        unique_names.len(),
        3,
        "the three Run overloads must not collide under the same Dart name: {method_names:?}"
    );
}

/// Criterion 3: a virtual call through a reference to the base class is
/// recorded and marked as dynamic dispatch. Criterion 5: callers of a
/// function can be listed from its definition. Criterion 6: a call that
/// isn't statically resolvable (here, through a function pointer) is
/// recorded and marked as such, not omitted.
#[test]
fn extract_function_catalog_records_the_call_graph_with_libclang() {
    let workspace =
        TempWorkspace::new("function-catalog-calls").expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    let unit = write_fixture(&project_root);

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let usr_of = |name: &str, kind: FunctionDeclarationKind| -> String {
        catalog
            .declarations
            .iter()
            .find(|declaration| declaration.name == name && declaration.kind == kind)
            .unwrap_or_else(|| {
                panic!(
                    "expected {name} ({kind:?}) in catalog: {:#?}",
                    catalog.declarations
                )
            })
            .usr
            .clone()
    };

    let shape_area = usr_of("area", FunctionDeclarationKind::Method);
    // `Shape::area` is the base declaration; `Circle::area` overrides it.
    // Whichever the fixture happens to list first, resolve both usrs so the
    // assertions below are order-independent.
    let area_usrs: Vec<&str> = catalog
        .declarations
        .iter()
        .filter(|declaration| declaration.name == "area")
        .map(|declaration| declaration.usr.as_str())
        .collect();
    assert_eq!(area_usrs.len(), 2);

    let describe_usr = usr_of("describe", FunctionDeclarationKind::FreeFunction);
    let add_int_usr = catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "add" && declaration.signature.contains("int"))
        .expect("expected add(int, int) overload")
        .usr
        .clone();
    let apply_usr = usr_of("apply", FunctionDeclarationKind::FreeFunction);
    let compute_usr = usr_of("compute", FunctionDeclarationKind::FreeFunction);

    // Criterion 3: `describe`'s `shape.area()` is a virtual call through a
    // base-class reference — resolved to a `Shape`/`Circle` `area` usr
    // (statically, the base declaration) and flagged as dynamic dispatch.
    let area_call = catalog
        .calls
        .iter()
        .find(|call| call.caller_usr == describe_usr)
        .unwrap_or_else(|| panic!("expected a call from describe: {:#?}", catalog.calls));
    match &area_call.resolution {
        CallResolution::Resolved {
            callee_usr,
            is_dynamic_dispatch,
        } => {
            assert!(
                area_usrs.contains(&callee_usr.as_str()),
                "expected the virtual call to resolve to one of area's usrs: {callee_usr}"
            );
            assert!(
                *is_dynamic_dispatch,
                "expected shape.area() to be marked as dynamic dispatch"
            );
        }
        other => panic!("expected a resolved, dynamically-dispatched call: {other:?}"),
    }
    let _ = shape_area;

    // Criterion 5: from `add(int,int)`'s definition, its one caller
    // (`compute`, via `add(1, 2)`) is listed.
    let add_callers: Vec<_> = catalog
        .calls
        .iter()
        .filter(|call| {
            matches!(
                &call.resolution,
                CallResolution::Resolved { callee_usr, .. } if *callee_usr == add_int_usr
            )
        })
        .collect();
    assert_eq!(
        add_callers.len(),
        1,
        "expected exactly one caller of add(int,int): {add_callers:#?}"
    );
    assert_eq!(add_callers[0].caller_usr, compute_usr);

    // `compute` also calls `describe` and `apply` directly (both statically
    // resolvable, non-virtual).
    let compute_calls: Vec<_> = catalog
        .calls
        .iter()
        .filter(|call| call.caller_usr == compute_usr)
        .collect();
    let resolves_to = |callee_usr: &str| {
        compute_calls.iter().any(|call| {
            matches!(
                &call.resolution,
                CallResolution::Resolved { callee_usr: actual, is_dynamic_dispatch: false }
                    if actual == callee_usr
            )
        })
    };
    assert!(
        resolves_to(&describe_usr),
        "expected compute to statically call describe: {compute_calls:#?}"
    );
    assert!(
        resolves_to(&apply_usr),
        "expected compute to statically call apply: {compute_calls:#?}"
    );

    // Criterion 6: `apply`'s `op(x, y)` goes through a function-pointer
    // parameter — not statically resolvable — and must be recorded as such,
    // not silently dropped.
    let apply_calls: Vec<_> = catalog
        .calls
        .iter()
        .filter(|call| call.caller_usr == apply_usr)
        .collect();
    assert_eq!(
        apply_calls.len(),
        1,
        "expected exactly one call recorded inside apply: {apply_calls:#?}"
    );
    match &apply_calls[0].resolution {
        CallResolution::Unresolved { reason } => {
            assert!(!reason.is_empty(), "expected a non-empty unresolved reason");
        }
        other => panic!("expected the indirect call through `op` to be unresolved: {other:?}"),
    }
}

/// A diamond-shaped multiple-inheritance case: `Square` overrides `area()`
/// from two unrelated bases (`Drawable` and `Measurable`) that happen to
/// declare a same-signature virtual method. Both bases give `area()` a body
/// (not `= 0`) so they get their own catalog entry too (a pure virtual has
/// no definition and, per US-5's "sem definição, sem entrada", wouldn't be
/// cataloged at all — that's not what this test is exercising). Kept as its
/// own fixture, separate from `FUNCTIONS_CPP`, so this test doesn't perturb
/// that other fixture's exact declaration counts.
const MULTIPLE_INHERITANCE_CPP: &str = r#"
class Drawable {
public:
    virtual double area() const { return 0.0; }
    virtual ~Drawable() {}
};

class Measurable {
public:
    virtual double area() const { return 0.0; }
    virtual ~Measurable() {}
};

class Square : public Drawable, public Measurable {
public:
    explicit Square(double side) : side_(side) {}
    double area() const override { return side_ * side_; }

private:
    double side_;
};
"#;

fn write_multiple_inheritance_fixture(project_root: &Path) -> CompilationUnit {
    fs::create_dir_all(project_root).expect("create project dir");
    let file_path = project_root.join("diamond.cpp");
    fs::write(&file_path, MULTIPLE_INHERITANCE_CPP).expect("write diamond.cpp");

    CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    }
}

/// US-5's open item on multiple inheritance: `Square::area` overrides
/// same-signature virtuals from two unrelated bases, and both must be
/// recorded — not just the first `clang_getOverriddenCursors` happens to
/// return.
#[test]
fn extract_function_catalog_records_every_overridden_base_under_multiple_inheritance() {
    let workspace = TempWorkspace::new("function-catalog-multiple-inheritance")
        .expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    let unit = write_multiple_inheritance_fixture(&project_root);

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let area_declarations: Vec<_> = catalog
        .declarations
        .iter()
        .filter(|declaration| declaration.name == "area")
        .collect();
    assert_eq!(
        area_declarations.len(),
        3,
        "expected Drawable::area, Measurable::area and Square::area as three distinct \
         declarations: {area_declarations:#?}"
    );

    // Exactly one of the three (Square's) overrides anything; the other two
    // are each other's bases, and override nothing themselves.
    let mut overriding: Vec<_> = area_declarations
        .iter()
        .filter(|declaration| !declaration.overridden_usrs.is_empty())
        .collect();
    assert_eq!(
        overriding.len(),
        1,
        "expected exactly one area() declaration (Square's) to override anything: \
         {area_declarations:#?}"
    );
    let square_area = overriding.remove(0);

    assert_eq!(
        square_area.overridden_usrs.len(),
        2,
        "expected Square::area to override both base area() methods: {square_area:#?}"
    );

    let base_usrs: Vec<&str> = area_declarations
        .iter()
        .filter(|declaration| declaration.usr != square_area.usr)
        .map(|declaration| declaration.usr.as_str())
        .collect();
    assert_eq!(base_usrs.len(), 2);
    for base_usr in base_usrs {
        assert!(
            square_area
                .overridden_usrs
                .iter()
                .any(|usr| usr == base_usr),
            "expected {base_usr} among Square::area's overridden usrs: {:#?}",
            square_area.overridden_usrs
        );
    }
}

/// A free function template and a method template — US-5's open item on
/// templates: both used to be invisible to the catalog (their cursor kind,
/// `CXCursor_FunctionTemplate`, wasn't in `function_declaration_kind_for`'s
/// match). `use_templates` calls both, once each, so the fixture also
/// exercises that a call to a template resolves back to the primary
/// template declaration, not to an untracked implicit instantiation.
const TEMPLATES_CPP: &str = r#"
template <typename T>
T templated(T a, T b) {
    return a + b;
}

struct Box {
    template <typename T>
    T identity(T value) { return value; }
};

int use_templates() {
    int sum = templated(1, 2);
    Box box;
    int same = box.identity(3);
    return sum + same;
}
"#;

fn write_templates_fixture(project_root: &Path) -> CompilationUnit {
    fs::create_dir_all(project_root).expect("create project dir");
    let file_path = project_root.join("templates.cpp");
    fs::write(&file_path, TEMPLATES_CPP).expect("write templates.cpp");

    CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    }
}

#[test]
fn extract_function_catalog_lists_function_and_method_templates_by_their_primary_declaration() {
    let workspace =
        TempWorkspace::new("function-catalog-templates").expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    let unit = write_templates_fixture(&project_root);

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let templated = catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "templated")
        .unwrap_or_else(|| {
            panic!(
                "expected the `templated` function template in the catalog: {:#?}",
                catalog.declarations
            )
        });
    assert_eq!(templated.kind, FunctionDeclarationKind::FunctionTemplate);
    assert_eq!(templated.owning_class_usr, None);
    assert_eq!(templated.signature, "T templated(T a, T b)");

    let identity = catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "identity")
        .unwrap_or_else(|| {
            panic!(
                "expected the `Box::identity` method template in the catalog: {:#?}",
                catalog.declarations
            )
        });
    assert_eq!(identity.kind, FunctionDeclarationKind::FunctionTemplate);
    assert!(
        identity.owning_class_usr.is_some(),
        "expected identity's owning class to be Box: {identity:#?}"
    );
    assert_eq!(identity.signature, "T Box::identity(T value)");

    // Both templates are called exactly once from `use_templates` — the
    // call must resolve back to the primary template's own usr, the one
    // the catalog entries above carry, not to an untracked implicit
    // instantiation with a different usr.
    let use_templates_usr = catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "use_templates")
        .expect("expected use_templates in the catalog")
        .usr
        .clone();

    let calls_from_use_templates: Vec<_> = catalog
        .calls
        .iter()
        .filter(|call| call.caller_usr == use_templates_usr)
        .collect();

    let resolves_to = |callee_usr: &str| {
        calls_from_use_templates.iter().any(|call| {
            matches!(
                &call.resolution,
                CallResolution::Resolved { callee_usr: actual, .. } if actual == callee_usr
            )
        })
    };
    assert!(
        resolves_to(&templated.usr),
        "expected a call resolving to templated's own usr: {calls_from_use_templates:#?}"
    );
    assert!(
        resolves_to(&identity.usr),
        "expected a call resolving to identity's own usr: {calls_from_use_templates:#?}"
    );
}

/// `docs/plans/lista-de-externos.md`: a free function declared but never
/// defined in any compilation unit of this fixture is still cataloged, with
/// `has_definition: false` and a real `ir::Function` (correct return type,
/// synthesized `Unsupported` body) — the auto-detection signal the
/// "extern" list needs to offer a mock for a symbol this project never
/// compiles a body for. A system-header prototype (`std::printf`, pulled in
/// by `<cstdio>`) must **not** be cataloged the same way — that gate is
/// what keeps the catalog from flooding with every libc declaration reached
/// through an `#include`.
const UNDEFINED_CALLEE_CPP: &str = r#"
#include <cstdio>

int NeverDefined(int x);

int caller() {
    printf("hello\n");
    return NeverDefined(4);
}
"#;

fn write_undefined_callee_fixture(project_root: &Path) -> CompilationUnit {
    fs::create_dir_all(project_root).expect("create project dir");
    let file_path = project_root.join("undefined_callee.cpp");
    fs::write(&file_path, UNDEFINED_CALLEE_CPP).expect("write undefined_callee.cpp");

    CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    }
}

#[test]
fn a_declared_but_never_defined_free_function_is_cataloged_with_a_mockable_signature() {
    let workspace = TempWorkspace::new("function-catalog-undefined-callee")
        .expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    let unit = write_undefined_callee_fixture(&project_root);

    let catalog = function_catalog::extract_function_catalog(
        std::slice::from_ref(&unit),
        &project_root,
        None,
    )
    .expect("extract function catalog");

    let never_defined = catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "NeverDefined")
        .unwrap_or_else(|| {
            panic!(
                "expected NeverDefined to be cataloged despite having no \
                 definition: {:#?}",
                catalog.declarations
            )
        });
    assert!(
        !never_defined.has_definition,
        "expected has_definition == false for a prototype-only function: \
         {never_defined:#?}"
    );

    let never_defined_ir = catalog
        .ir_functions
        .iter()
        .find(|function| function.usr == never_defined.usr)
        .unwrap_or_else(|| {
            panic!(
                "expected an ir::Function synthesized for the prototype-only \
                 usr {:?}: {:#?}",
                never_defined.usr, catalog.ir_functions
            )
        });
    assert_eq!(
        never_defined_ir.return_type,
        syntax_bridge_server::ir::Type::Int
    );
    assert_eq!(
        never_defined_ir.body.len(),
        1,
        "expected a single synthesized bailout statement: {:#?}",
        never_defined_ir.body
    );
    assert!(
        matches!(
            &never_defined_ir.body[0],
            syntax_bridge_server::ir::Stmt::Unsupported { .. }
        ),
        "expected the synthesized body to be an honest Unsupported bailout \
         (only relevant if this usr ever falls outside the effective \
         external set): {:#?}",
        never_defined_ir.body
    );

    // F6/tarefa 07, Metade B: unlike an in-project prototype (cataloged
    // from the top-level declaration walk, gated to `!is_system_header` so
    // the flood this comment used to warn about never happens), a
    // system-header free function is only ever cataloged the moment a call
    // site actually resolves to it — `printf` here is genuinely *called*,
    // so it earns a real, named, mockable Dart adapter (`externals.rs`'s
    // `AutoUndefinedFunction` auto-detection is what turns this into a
    // named external boundary at the emit step, `emit::dart::
    // emit_module_with_externals`'s own scope, not this catalog step's).
    let printf_declaration = catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "printf")
        .unwrap_or_else(|| {
            panic!(
                "expected printf to be cataloged, since the fixture calls it: {:#?}",
                catalog.declarations
            )
        });
    assert!(
        !printf_declaration.has_definition,
        "expected has_definition == false for a system-header symbol this project never \
         defines: {printf_declaration:#?}"
    );
    assert!(
        catalog
            .ir_functions
            .iter()
            .any(|function| function.usr == printf_declaration.usr),
        "expected an ir::Function synthesized for printf's usr {:?}: {:#?}",
        printf_declaration.usr,
        catalog.ir_functions
    );
}

/// Companion to the test above: when the *same* usr is a prototype-only
/// sighting in one compilation unit and a real definition in another, the
/// merge must keep the definition — never the reverse, and never both.
const SHARED_HEADER_H: &str = r#"
int Shared(int x);
"#;

const DECLARES_ONLY_CPP: &str = r#"
#include "shared.h"

int caller() {
    return Shared(1);
}
"#;

const DEFINES_SHARED_CPP: &str = r#"
#include "shared.h"

int Shared(int x) {
    return x * 2;
}
"#;

fn write_split_prototype_and_definition_fixture(project_root: &Path) -> Vec<CompilationUnit> {
    fs::create_dir_all(project_root).expect("create project dir");
    fs::write(project_root.join("shared.h"), SHARED_HEADER_H).expect("write shared.h");
    let declares_only = project_root.join("declares_only.cpp");
    fs::write(&declares_only, DECLARES_ONLY_CPP).expect("write declares_only.cpp");
    let defines_shared = project_root.join("defines_shared.cpp");
    fs::write(&defines_shared, DEFINES_SHARED_CPP).expect("write defines_shared.cpp");

    vec![
        CompilationUnit {
            directory: project_root.display().to_string(),
            file: declares_only.display().to_string(),
            command: None,
            arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
        },
        CompilationUnit {
            directory: project_root.display().to_string(),
            file: defines_shared.display().to_string(),
            command: None,
            arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
        },
    ]
}

#[test]
fn a_prototype_seen_in_one_compilation_unit_is_upgraded_to_the_definition_found_in_another() {
    let workspace = TempWorkspace::new("function-catalog-prototype-upgrade")
        .expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    let units = write_split_prototype_and_definition_fixture(&project_root);

    let catalog = function_catalog::extract_function_catalog(&units, &project_root, None)
        .expect("extract function catalog");

    let shared_declarations: Vec<_> = catalog
        .declarations
        .iter()
        .filter(|declaration| declaration.name == "Shared")
        .collect();
    assert_eq!(
        shared_declarations.len(),
        1,
        "expected exactly one Shared declaration after merge, not one per \
         compilation unit: {shared_declarations:#?}"
    );
    assert!(
        shared_declarations[0].has_definition,
        "expected the merge to prefer the real definition over the \
         prototype-only sighting: {:#?}",
        shared_declarations[0]
    );

    let shared_usr = shared_declarations[0].usr.clone();
    let shared_ir: Vec<_> = catalog
        .ir_functions
        .iter()
        .filter(|function| function.usr == shared_usr)
        .collect();
    assert_eq!(
        shared_ir.len(),
        1,
        "expected exactly one ir::Function for Shared after merge: {shared_ir:#?}"
    );
    assert!(
        !matches!(
            shared_ir[0].body.as_slice(),
            [syntax_bridge_server::ir::Stmt::Unsupported { .. }]
        ),
        "expected the real body (`return x * 2;`), not the synthesized \
         prototype bailout: {:#?}",
        shared_ir[0].body
    );
}

struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    fn new(name: &str) -> io::Result<Self> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "syntax-bridge-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&path)?;

        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}
