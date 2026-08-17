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

    assert!(
        !catalog
            .declarations
            .iter()
            .any(|declaration| declaration.name == "printf"),
        "a system-header prototype must never be cataloged just because it \
         was declared, or every libc symbol reachable through an #include \
         would flood the catalog: {:#?}",
        catalog.declarations
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
