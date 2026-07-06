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

use syntax_bridge_server::ingest::CreateProjectRequest;
use syntax_bridge_server::project_service;
use syntax_bridge_server::type_catalog::{TypeDeclaration, TypeDeclarationKind};

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
    )
    .expect("ingest project and extract type catalog");

    let catalog = &project.type_catalog;

    let find = |name: &str, kind: TypeDeclarationKind| {
        catalog
            .iter()
            .find(|declaration| declaration.name == name && declaration.kind == kind)
    };

    let macro_decl = find("ANSWER", TypeDeclarationKind::Macro)
        .unwrap_or_else(|| panic!("expected ANSWER macro in catalog: {catalog:#?}"));
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
