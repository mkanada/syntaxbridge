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
