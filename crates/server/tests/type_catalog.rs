//! Exercises the real `libclang` type-catalog extraction end to end, through
//! `project_service::create_project`.
//!
//! This depends on a real `libclang` shared library being loadable in the
//! current environment (see `crates/server/src/type_catalog.rs`), so it must
//! be run inside the Flatpak sandbox via `scripts/test-in-flatpak.sh`, which
//! activates the `llvm21` SDK extension.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use syntax_bridge_server::ingest::{CompilationUnit, CreateProjectRequest};
use syntax_bridge_server::progress::ExtractionProgress;
use syntax_bridge_server::project_service;
use syntax_bridge_server::type_catalog::{
    self, TypeDeclaration, TypeDeclarationKind, TypeUsage, TypeUsageKind,
};

const MAIN_CPP: &str = r#"
#include "types.h"

int main() {
    Point origin{0, 0};
    Widget widget;
    Number number;
    AliasInt total = ANSWER;
    (void)origin;
    (void)widget;
    (void)number;
    return total == ANSWER ? 0 : 1;
}
"#;

const OTHER_CPP: &str = r#"
#include "types.h"

Point make_origin() {
    return Point{0, 0};
}
"#;

const TYPES_H: &str = r#"
#define ANSWER 42

typedef int MyInt;
using AliasInt = int;

struct Point {
    int x;
    int y;
};

enum Color { RED, GREEN, BLUE };

class Widget {
public:
    int id;
};

union Number {
    int i;
    float f;
};
"#;

#[test]
fn create_project_catalogs_project_types_with_libclang() {
    let workspace = TempWorkspace::new("type-catalog").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "type_catalog_fixture".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project and extract type catalog");

    let catalog = &project.type_catalog;

    let find = |name: &str, kind: TypeDeclarationKind| {
        catalog
            .iter()
            .find(|declaration| declaration.name == name && declaration.kind == kind)
    };

    let macro_decl = find("ANSWER", TypeDeclarationKind::ConstantMacro)
        .unwrap_or_else(|| panic!("expected ANSWER constant macro in catalog: {catalog:#?}"));
    assert!(macro_decl.file.ends_with("types.h"));

    let typedef_decl = find("MyInt", TypeDeclarationKind::Typedef)
        .unwrap_or_else(|| panic!("expected MyInt typedef in catalog: {catalog:#?}"));
    assert!(typedef_decl.file.ends_with("types.h"));

    let alias_decl = find("AliasInt", TypeDeclarationKind::TypeAlias)
        .unwrap_or_else(|| panic!("expected AliasInt type alias in catalog: {catalog:#?}"));
    assert!(alias_decl.file.ends_with("types.h"));

    let struct_decl = find("Point", TypeDeclarationKind::Struct)
        .unwrap_or_else(|| panic!("expected Point struct in catalog: {catalog:#?}"));
    assert!(struct_decl.file.ends_with("types.h"));

    let enum_decl = find("Color", TypeDeclarationKind::Enum)
        .unwrap_or_else(|| panic!("expected Color enum in catalog: {catalog:#?}"));
    assert!(enum_decl.file.ends_with("types.h"));

    let class_decl = find("Widget", TypeDeclarationKind::Class)
        .unwrap_or_else(|| panic!("expected Widget class in catalog: {catalog:#?}"));
    assert!(class_decl.file.ends_with("types.h"));

    let union_decl = find("Number", TypeDeclarationKind::Union)
        .unwrap_or_else(|| panic!("expected Number union in catalog: {catalog:#?}"));
    assert!(union_decl.file.ends_with("types.h"));

    let point_occurrences = catalog
        .iter()
        .filter(|declaration| {
            declaration.name == "Point" && declaration.kind == TypeDeclarationKind::Struct
        })
        .count();
    assert_eq!(
        point_occurrences, 1,
        "Point is included from two translation units but should be deduplicated: {catalog:#?}"
    );

    let project_store = syntax_bridge_server::persistence::ProjectStore::open(
        &project.project_dir.join("project.db"),
    )
    .expect("open project store");
    let persisted = project_store
        .list_type_declarations()
        .expect("list persisted type declarations");
    assert_eq!(persisted.len(), catalog.len());
    for declaration in catalog {
        assert!(
            persisted.contains(declaration),
            "expected persisted catalog to contain {declaration:?}"
        );
    }
}

const DEPS_MAIN_CPP: &str = r#"
#include "types.h"

int main() {
    Rect rect{};
    Panel panel;
    PointAlias origin{};
    WidgetAlias widget;
    MyInt count = 0;
    (void)rect;
    (void)panel;
    (void)origin;
    (void)widget;
    (void)count;
    return 0;
}
"#;

const DEPS_TYPES_H: &str = r#"
struct Point {
    int x;
    int y;
};

struct Rect {
    Point top_left;
    Point bottom_right;
};

class Widget {
public:
    int id;
};

class Panel : public Widget {
public:
    Rect bounds;
};

typedef int MyInt;
typedef Point PointAlias;
using WidgetAlias = Widget;
"#;

#[test]
fn create_project_extracts_type_dependencies_with_libclang() {
    let workspace = TempWorkspace::new("type-catalog-deps").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_dependencies_fixture_tarball(workspace.path(), &archive_path)
        .expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "type_dependencies_fixture".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project and extract type dependencies");

    let catalog = &project.type_catalog;
    let dependencies = &project.type_dependencies;

    let find = |name: &str, kind: TypeDeclarationKind| {
        catalog
            .iter()
            .find(|declaration| declaration.name == name && declaration.kind == kind)
            .unwrap_or_else(|| panic!("expected {name} in catalog: {catalog:#?}"))
            .clone()
    };

    let point = find("Point", TypeDeclarationKind::Struct);
    let rect = find("Rect", TypeDeclarationKind::Struct);
    let widget = find("Widget", TypeDeclarationKind::Class);
    let panel = find("Panel", TypeDeclarationKind::Class);
    let point_alias = find("PointAlias", TypeDeclarationKind::Typedef);
    let widget_alias = find("WidgetAlias", TypeDeclarationKind::TypeAlias);
    let my_int = find("MyInt", TypeDeclarationKind::Typedef);

    let has_edge = |caller: &TypeDeclaration, callee: &TypeDeclaration| {
        dependencies
            .iter()
            .any(|dependency| &dependency.caller == caller && &dependency.callee == callee)
    };

    assert!(
        has_edge(&rect, &point),
        "expected Rect -> Point dependency: {dependencies:#?}"
    );
    assert!(
        has_edge(&panel, &widget),
        "expected Panel -> Widget (base class) dependency: {dependencies:#?}"
    );
    assert!(
        has_edge(&panel, &rect),
        "expected Panel -> Rect (field) dependency: {dependencies:#?}"
    );
    assert!(
        has_edge(&point_alias, &point),
        "expected PointAlias -> Point dependency: {dependencies:#?}"
    );
    assert!(
        has_edge(&widget_alias, &widget),
        "expected WidgetAlias -> Widget dependency: {dependencies:#?}"
    );

    let rect_to_point_edges = dependencies
        .iter()
        .filter(|dependency| dependency.caller == rect && dependency.callee == point)
        .count();
    assert_eq!(
        rect_to_point_edges, 1,
        "Rect has two Point fields but the dependency edge should be deduplicated: {dependencies:#?}"
    );

    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency.caller == my_int),
        "MyInt's underlying type is a builtin with no declaration, so it should have no outgoing edge: {dependencies:#?}"
    );

    let dependencies_module = syntax_bridge_server::persistence::ProjectStore::open(
        &project.project_dir.join("project.db"),
    )
    .expect("open project store");
    let persisted = dependencies_module
        .list_type_dependencies()
        .expect("list persisted type dependencies");
    assert_eq!(persisted.len(), dependencies.len());
    for dependency in dependencies {
        assert!(
            persisted.contains(dependency),
            "expected persisted dependencies to contain {dependency:?}"
        );
    }
}

const NAMESPACES_MAIN_CPP: &str = r#"
#include "types.h"

int main() {
    Point top_level{};
    geometry::Point nested{};
    geometry::detail::Helper helper{};
    (void)top_level;
    (void)nested;
    (void)helper;
    return 0;
}
"#;

const NAMESPACES_TYPES_H: &str = r#"
struct Point {
    int x;
};

namespace geometry {

struct Point {
    int x;
    int y;
};

namespace detail {

class Helper {
public:
    int value;
};

}  // namespace detail

}  // namespace geometry
"#;

#[test]
fn create_project_catalogs_namespace_and_extent_with_libclang() {
    let workspace =
        TempWorkspace::new("type-catalog-namespaces").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_namespaces_fixture_tarball(workspace.path(), &archive_path)
        .expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "type_catalog_namespaces_fixture".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project and extract type catalog");

    let catalog = &project.type_catalog;

    let top_level_point = catalog
        .iter()
        .find(|declaration| {
            declaration.name == "Point"
                && declaration.kind == TypeDeclarationKind::Struct
                && declaration.namespace.is_empty()
        })
        .unwrap_or_else(|| panic!("expected global-scope Point in catalog: {catalog:#?}"));

    let nested_point = catalog
        .iter()
        .find(|declaration| {
            declaration.name == "Point"
                && declaration.kind == TypeDeclarationKind::Struct
                && declaration.namespace == "geometry"
        })
        .unwrap_or_else(|| panic!("expected geometry::Point in catalog: {catalog:#?}"));

    assert_ne!(
        top_level_point, nested_point,
        "global-scope Point and geometry::Point are distinct types and must not collapse into one entry"
    );

    let helper = catalog
        .iter()
        .find(|declaration| declaration.name == "Helper")
        .unwrap_or_else(|| panic!("expected Helper in catalog: {catalog:#?}"));
    assert_eq!(
        helper.namespace, "geometry::detail",
        "expected Helper's namespace to include both enclosing namespaces: {helper:#?}"
    );

    assert!(
        nested_point.end_line > nested_point.line,
        "expected geometry::Point's extent to cover its multi-line body: {nested_point:#?}"
    );
}

const MACROS_MAIN_CPP: &str = r#"
#include "macros.h"

int main() {
    int limit = MAX_SIZE;
    int squared = SQUARE(limit);
    (void)limit;
    (void)squared;
    return 0;
}
"#;

const MACROS_H: &str = r#"
#ifndef MACROS_H
#define MACROS_H

#define MAX_SIZE 100
#define SQUARE(x) ((x) * (x))
#define MYLIB_API

MYLIB_API int compute();

#endif
"#;

/// The type catalog should be selective about what counts as a "type":
/// header guards and valueless annotation macros are pure preprocessor
/// noise with nothing to show a user, so they get their own kinds instead
/// of being lumped into a generic `Macro` alongside real constants and
/// function-like macros.
#[test]
fn create_project_classifies_macro_kinds_with_libclang() {
    let workspace = TempWorkspace::new("type-catalog-macros").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_macros_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "type_catalog_macros_fixture".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project and extract type catalog");

    let catalog = &project.type_catalog;

    let find = |name: &str, kind: TypeDeclarationKind| {
        catalog
            .iter()
            .find(|declaration| declaration.name == name && declaration.kind == kind)
    };

    find("MAX_SIZE", TypeDeclarationKind::ConstantMacro).unwrap_or_else(|| {
        panic!("expected MAX_SIZE classified as a constant macro: {catalog:#?}")
    });

    find("SQUARE", TypeDeclarationKind::FunctionMacro).unwrap_or_else(|| {
        panic!("expected SQUARE classified as a function-like macro: {catalog:#?}")
    });

    find("MACROS_H", TypeDeclarationKind::HeaderGuard)
        .unwrap_or_else(|| panic!("expected MACROS_H classified as a header guard: {catalog:#?}"));

    find("MYLIB_API", TypeDeclarationKind::AnnotationMacro).unwrap_or_else(|| {
        panic!("expected MYLIB_API classified as a valueless annotation macro: {catalog:#?}")
    });
}

fn write_macros_fixture_tarball(workspace: &Path, archive_path: &Path) -> io::Result<()> {
    let source_dir = workspace.join("fixture");
    fs::create_dir_all(&source_dir)?;
    fs::write(source_dir.join("main.cpp"), MACROS_MAIN_CPP)?;
    fs::write(source_dir.join("macros.h"), MACROS_H)?;
    fs::write(
        source_dir.join("CMakeLists.txt"),
        r#"
cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_type_catalog_macros_fixture LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_executable(syntax_bridge_type_catalog_macros_fixture main.cpp)
"#,
    )?;

    let output = Command::new("tar")
        .arg("-czf")
        .arg(archive_path)
        .arg("-C")
        .arg(workspace)
        .arg("fixture")
        .output()?;
    assert_success(output);

    Ok(())
}

fn write_namespaces_fixture_tarball(workspace: &Path, archive_path: &Path) -> io::Result<()> {
    let source_dir = workspace.join("fixture");
    fs::create_dir_all(&source_dir)?;
    fs::write(source_dir.join("main.cpp"), NAMESPACES_MAIN_CPP)?;
    fs::write(source_dir.join("types.h"), NAMESPACES_TYPES_H)?;
    fs::write(
        source_dir.join("CMakeLists.txt"),
        r#"
cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_type_catalog_namespaces_fixture LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_executable(syntax_bridge_type_catalog_namespaces_fixture main.cpp)
"#,
    )?;

    let output = Command::new("tar")
        .arg("-czf")
        .arg(archive_path)
        .arg("-C")
        .arg(workspace)
        .arg("fixture")
        .output()?;
    assert_success(output);

    Ok(())
}

/// `GET /projects/jobs/{id}` needs to observe progress mid-extraction, not
/// just the before/after totals — this proves `extract_type_catalog` reports
/// through the tracker as it goes, using a plain two-file fixture (no
/// tarball/CMake ingest needed to exercise this).
#[test]
fn extract_type_catalog_reports_progress_as_units_complete() {
    let workspace =
        TempWorkspace::new("type-catalog-progress").expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");

    let file_a = project_root.join("a.cpp");
    let file_b = project_root.join("b.cpp");
    fs::write(&file_a, "struct A { int x; };").expect("write a.cpp");
    fs::write(&file_b, "struct B { int y; };").expect("write b.cpp");

    let units = vec![
        CompilationUnit {
            directory: project_root.display().to_string(),
            file: file_a.display().to_string(),
            command: None,
            arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
        },
        CompilationUnit {
            directory: project_root.display().to_string(),
            file: file_b.display().to_string(),
            command: None,
            arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
        },
    ];

    let progress = ExtractionProgress::new();
    assert_eq!(
        progress.total(),
        0,
        "total should read as unset before extraction starts"
    );

    type_catalog::extract_type_catalog(&units, &project_root, Some(&progress))
        .expect("extract type catalog");

    assert_eq!(progress.total(), 2, "expected total set to the unit count");
    assert_eq!(
        progress.completed(),
        2,
        "expected both units marked done by the end"
    );
}

/// The "identidade de tipo é frágil" open item in `docs/plans/User Steps.md`
/// (US-3): the previous `(kind, name, file, line, column)` key breaks the
/// instant an unrelated edit shifts line numbers. `clang_getCursorUSR` gives
/// libclang's own semantic identity for a cursor, independent of position,
/// so it survives exactly that kind of edit.
#[test]
fn type_declaration_usr_is_stable_across_incidental_line_shifts() {
    let workspace = TempWorkspace::new("type-catalog-usr").expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");

    let file_path = project_root.join("shapes.cpp");
    let before = "struct Shape {\n    int sides;\n};\n";
    fs::write(&file_path, before).expect("write shapes.cpp (before)");

    let unit = CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let before_catalog =
        type_catalog::extract_type_catalog(std::slice::from_ref(&unit), &project_root, None)
            .expect("extract catalog before edit");
    let before_shape = before_catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "Shape")
        .unwrap_or_else(|| panic!("expected Shape in catalog before edit: {before_catalog:#?}"));
    assert!(
        !before_shape.usr.is_empty(),
        "expected a non-empty USR: {before_shape:#?}"
    );

    // An incidental edit — two blank lines inserted above the struct — shifts
    // every following line number without changing what Shape *is*.
    let after = format!("\n\n{before}");
    fs::write(&file_path, &after).expect("write shapes.cpp (after)");

    let after_catalog = type_catalog::extract_type_catalog(&[unit], &project_root, None)
        .expect("extract catalog after edit");
    let after_shape = after_catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "Shape")
        .unwrap_or_else(|| panic!("expected Shape in catalog after edit: {after_catalog:#?}"));

    assert_ne!(
        before_shape.line, after_shape.line,
        "the edit should have shifted Shape's line number"
    );
    assert_eq!(
        before_shape.usr, after_shape.usr,
        "Shape's USR should stay stable across a line-shifting edit that doesn't change its identity"
    );
}

/// Complements the line-shift test above: two homonym types in different
/// namespaces are a distinct pair of identities, not just a distinct pair of
/// qualified display names, so their USRs must differ too.
#[test]
fn type_declaration_usr_distinguishes_namespaced_homonyms_with_libclang() {
    let workspace =
        TempWorkspace::new("type-catalog-usr-namespaces").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_namespaces_fixture_tarball(workspace.path(), &archive_path)
        .expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "type_catalog_usr_namespaces_fixture".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project and extract type catalog");

    let catalog = &project.type_catalog;

    let top_level_point = catalog
        .iter()
        .find(|declaration| declaration.name == "Point" && declaration.namespace.is_empty())
        .unwrap_or_else(|| panic!("expected global-scope Point in catalog: {catalog:#?}"));

    let nested_point = catalog
        .iter()
        .find(|declaration| declaration.name == "Point" && declaration.namespace == "geometry")
        .unwrap_or_else(|| panic!("expected geometry::Point in catalog: {catalog:#?}"));

    assert!(
        !top_level_point.usr.is_empty() && !nested_point.usr.is_empty(),
        "expected both Points to carry a non-empty USR: {top_level_point:#?} {nested_point:#?}"
    );
    assert_ne!(
        top_level_point.usr, nested_point.usr,
        "global-scope Point and geometry::Point are distinct types and must have distinct USRs"
    );
}

const USAGE_TAXONOMY_CPP: &str = r#"
struct Point {
    int x;
    int y;
};

struct Widget {
    int id;
};

struct Panel : public Widget {
    Point origin;
};

typedef Point PointAlias;

Point global_point;

Point make_point(Point seed) {
    return seed;
}
"#;

/// US-4's taxonomy of "uso": every signature-level mention of a project type
/// (field, parameter, return type, base class, file-scope variable, typedef
/// underlying type) is recorded as a `TypeUsage`, keyed by the used type's
/// USR (US-3) rather than position. Expression-level kinds that only appear
/// inside function bodies (casts, `sizeof`, `new`, template arguments) are
/// deliberately out of scope here — `type_catalog.rs` parses with
/// `CXTranslationUnit_SkipFunctionBodies` for performance (see the "Escala"
/// note in `docs/plans/User Steps.md`), and this pass reuses that same AST
/// walk instead of reparsing, so anything only visible inside a body isn't
/// visited at all.
#[test]
fn extract_type_catalog_records_usages_across_the_defined_taxonomy() {
    let workspace = TempWorkspace::new("type-catalog-usages").expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");

    let file_path = project_root.join("usages.cpp");
    fs::write(&file_path, USAGE_TAXONOMY_CPP).expect("write usages.cpp");

    let unit = CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let catalog =
        type_catalog::extract_type_catalog(std::slice::from_ref(&unit), &project_root, None)
            .expect("extract type catalog");

    let point = catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "Point")
        .unwrap_or_else(|| panic!("expected Point in catalog: {catalog:#?}"));
    let widget = catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "Widget")
        .unwrap_or_else(|| panic!("expected Widget in catalog: {catalog:#?}"));

    let point_usages: Vec<&TypeUsage> = catalog
        .usages
        .iter()
        .filter(|usage| usage.type_usr == point.usr)
        .collect();
    let widget_usages: Vec<&TypeUsage> = catalog
        .usages
        .iter()
        .filter(|usage| usage.type_usr == widget.usr)
        .collect();

    let find_usage = |usages: &[&TypeUsage], kind: TypeUsageKind| -> TypeUsage {
        usages
            .iter()
            .find(|usage| usage.kind == kind)
            .unwrap_or_else(|| panic!("expected a {kind:?} usage among {usages:#?}"))
            .to_owned()
            .clone()
    };

    let field = find_usage(&point_usages, TypeUsageKind::Field);
    let (field_line, field_column) = locate_in(USAGE_TAXONOMY_CPP, "Point origin;", "origin");
    assert_eq!(field.line, field_line, "field usage line: {field:#?}");
    assert_eq!(field.column, field_column, "field usage column: {field:#?}");
    assert!(field.file.ends_with("usages.cpp"));

    let typedef_mention = find_usage(&point_usages, TypeUsageKind::TypedefMention);
    let (typedef_line, typedef_column) = locate_in(
        USAGE_TAXONOMY_CPP,
        "typedef Point PointAlias;",
        "PointAlias",
    );
    assert_eq!(typedef_mention.line, typedef_line);
    assert_eq!(typedef_mention.column, typedef_column);

    let variable_declaration = find_usage(&point_usages, TypeUsageKind::VariableDeclaration);
    let (var_line, var_column) =
        locate_in(USAGE_TAXONOMY_CPP, "Point global_point;", "global_point");
    assert_eq!(variable_declaration.line, var_line);
    assert_eq!(variable_declaration.column, var_column);

    let return_type = find_usage(&point_usages, TypeUsageKind::ReturnType);
    let (return_line, return_column) = locate_in(
        USAGE_TAXONOMY_CPP,
        "Point make_point(Point seed) {",
        "make_point",
    );
    assert_eq!(return_type.line, return_line);
    assert_eq!(return_type.column, return_column);

    let parameter = find_usage(&point_usages, TypeUsageKind::Parameter);
    let (param_line, param_column) =
        locate_in(USAGE_TAXONOMY_CPP, "Point make_point(Point seed) {", "seed");
    assert_eq!(parameter.line, param_line);
    assert_eq!(parameter.column, param_column);

    let inheritance = find_usage(&widget_usages, TypeUsageKind::Inheritance);
    let (base_line, base_column) = locate_in(
        USAGE_TAXONOMY_CPP,
        "struct Panel : public Widget {",
        "Widget",
    );
    assert_eq!(inheritance.line, base_line);
    assert_eq!(inheritance.column, base_column);

    assert_eq!(
        point_usages.len(),
        5,
        "expected exactly 5 usages of Point (field, typedef mention, variable declaration, \
         return type, parameter): {point_usages:#?}"
    );
    assert_eq!(
        widget_usages.len(),
        1,
        "expected exactly 1 usage of Widget (inheritance): {widget_usages:#?}"
    );
}

/// Finds the 1-indexed `(line, column)` of `sub_needle` on the line
/// containing `line_marker`, so expected usage locations are computed from
/// the fixture text instead of hand-counted (and kept correct if the fixture
/// is edited later).
fn locate_in(content: &str, line_marker: &str, sub_needle: &str) -> (u32, u32) {
    for (line_index, line) in content.lines().enumerate() {
        if line.contains(line_marker) {
            let byte_offset = line.find(sub_needle).unwrap_or_else(|| {
                panic!("expected to find {sub_needle:?} on line containing {line_marker:?}")
            });
            let column = line[..byte_offset].chars().count() + 1;
            return (line_index as u32 + 1, column as u32);
        }
    }
    panic!("expected to find a line containing {line_marker:?}");
}

fn write_dependencies_fixture_tarball(workspace: &Path, archive_path: &Path) -> io::Result<()> {
    let source_dir = workspace.join("fixture");
    fs::create_dir_all(&source_dir)?;
    fs::write(source_dir.join("main.cpp"), DEPS_MAIN_CPP)?;
    fs::write(source_dir.join("types.h"), DEPS_TYPES_H)?;
    fs::write(
        source_dir.join("CMakeLists.txt"),
        r#"
cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_type_dependencies_fixture LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_executable(syntax_bridge_type_dependencies_fixture main.cpp)
"#,
    )?;

    let output = Command::new("tar")
        .arg("-czf")
        .arg(archive_path)
        .arg("-C")
        .arg(workspace)
        .arg("fixture")
        .output()?;
    assert_success(output);

    Ok(())
}

fn write_fixture_tarball(workspace: &Path, archive_path: &Path) -> io::Result<()> {
    let source_dir = workspace.join("fixture");
    fs::create_dir_all(&source_dir)?;
    fs::write(source_dir.join("main.cpp"), MAIN_CPP)?;
    fs::write(source_dir.join("other.cpp"), OTHER_CPP)?;
    fs::write(source_dir.join("types.h"), TYPES_H)?;
    fs::write(
        source_dir.join("CMakeLists.txt"),
        r#"
cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_type_catalog_fixture LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_executable(syntax_bridge_type_catalog_fixture main.cpp other.cpp)
"#,
    )?;

    let output = Command::new("tar")
        .arg("-czf")
        .arg(archive_path)
        .arg("-C")
        .arg(workspace)
        .arg("fixture")
        .output()?;
    assert_success(output);

    Ok(())
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[derive(Debug)]
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

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
