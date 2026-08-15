//! Exercises the real `libclang` pointer-catalog extraction (Parte 1 of
//! `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`): every raw `T*`
//! pointer declared across a project's compilation units — parameter, field,
//! local variable, function return type — plus its shape (plain, `T**`, or a
//! function pointer) and, when the pointee is a type this project's own
//! `type_catalog` already tracks, that pointee's stable `usr`.
//!
//! Like `type_catalog.rs`, this depends on a real `libclang` shared library
//! being loadable in the current environment, so it must be run inside the
//! Flatpak sandbox via `scripts/test-in-flatpak.sh`.

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use syntax_bridge_server::ingest::{CompilationUnit, CreateProjectRequest};
use syntax_bridge_server::pointer_catalog::{
    self, PointerDeclaration, PointerDeclarationKind, PointerShape,
};
use syntax_bridge_server::project_service;
use syntax_bridge_server::type_catalog;

const POINTERS_CPP: &str = r#"
struct Forma {
    int lados;
};

void desenha(Forma* forma) {
    Forma* local = forma;
    Forma** duplo = &local;
    void (*callback)(int) = nullptr;
    (void)local;
    (void)duplo;
    (void)callback;
}

struct Painel {
    Forma* atual;
};

Forma* fabrica();
"#;

/// Covers the closed taxonomy this pass tracks: `parameter`, `local`,
/// `field`, `return_type` for the site, and `scalar`/`double_pointer`/
/// `function_pointer` for the shape — plus resolving a known pointee back to
/// `type_catalog`'s own `usr` for `Forma`, and leaving it empty for the
/// function-pointer case, which has no named-type pointee to resolve.
#[test]
fn extracts_every_pointer_declaration_across_the_defined_taxonomy() {
    let workspace =
        TempWorkspace::new("pointer-catalog-taxonomy").expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");

    let file_path = project_root.join("pointers.cpp");
    fs::write(&file_path, POINTERS_CPP).expect("write pointers.cpp");

    let unit = CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let type_catalog =
        type_catalog::extract_type_catalog(std::slice::from_ref(&unit), &project_root, None)
            .expect("extract type catalog");
    let forma = type_catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "Forma")
        .unwrap_or_else(|| panic!("expected Forma in type catalog: {type_catalog:#?}"));
    assert!(!forma.usr.is_empty(), "expected Forma to carry a usr");

    let pointers =
        pointer_catalog::extract_pointer_catalog(std::slice::from_ref(&unit), &project_root, None)
            .expect("extract pointer catalog");

    let find = |kind: PointerDeclarationKind, name: &str| -> PointerDeclaration {
        pointers
            .iter()
            .find(|declaration| declaration.kind == kind && declaration.name == name)
            .unwrap_or_else(|| panic!("expected {kind:?} {name:?} among {pointers:#?}"))
            .clone()
    };

    let parameter = find(PointerDeclarationKind::Parameter, "forma");
    assert_eq!(parameter.shape, PointerShape::Scalar);
    assert_eq!(parameter.pointee_type_name.trim(), "Forma");
    assert_eq!(parameter.pointee_usr, forma.usr);
    assert!(parameter.file.ends_with("pointers.cpp"));

    let local = find(PointerDeclarationKind::Local, "local");
    assert_eq!(local.shape, PointerShape::Scalar);
    assert_eq!(local.pointee_usr, forma.usr);

    let double_pointer = find(PointerDeclarationKind::Local, "duplo");
    assert_eq!(double_pointer.shape, PointerShape::DoublePointer);
    assert_eq!(
        double_pointer.pointee_usr, forma.usr,
        "T** should still resolve through both indirections to Forma's usr"
    );

    let function_pointer = find(PointerDeclarationKind::Local, "callback");
    assert_eq!(function_pointer.shape, PointerShape::FunctionPointer);
    assert!(
        function_pointer.pointee_usr.is_empty(),
        "a function pointer has no named-type pointee to resolve: {function_pointer:#?}"
    );

    let field = find(PointerDeclarationKind::Field, "atual");
    assert_eq!(field.shape, PointerShape::Scalar);
    assert_eq!(field.pointee_usr, forma.usr);

    let return_type = find(PointerDeclarationKind::ReturnType, "fabrica");
    assert_eq!(return_type.shape, PointerShape::Scalar);
    assert_eq!(return_type.pointee_usr, forma.usr);

    let local_count = pointers
        .iter()
        .filter(|declaration| declaration.kind == PointerDeclarationKind::Local)
        .count();
    assert_eq!(
        local_count, 3,
        "expected exactly 3 local pointers (local, duplo, callback): {pointers:#?}"
    );
}

const PROJECT_MAIN_CPP: &str = r#"
#include "types.h"

Forma* fabrica();

int main() {
    Forma* origem = fabrica();
    (void)origem;
    return 0;
}
"#;

const PROJECT_TYPES_H: &str = r#"
struct Forma {
    int lados;
};
"#;

/// End-to-end through `project_service::create_project`, the same seam
/// `type_catalog.rs`'s first test exercises: proves the pointer catalog is
/// extracted, attached to the returned project, and persisted to
/// `project.db` — not just that `extract_pointer_catalog` works in
/// isolation.
#[test]
fn create_project_catalogs_project_pointers_with_libclang() {
    let workspace =
        TempWorkspace::new("pointer-catalog-project").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "pointer_catalog_fixture".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project and extract pointer catalog");

    let catalog = &project.pointer_catalog;
    let return_type_pointer = catalog
        .iter()
        .find(|declaration| {
            declaration.kind == PointerDeclarationKind::ReturnType && declaration.name == "fabrica"
        })
        .unwrap_or_else(|| {
            panic!("expected fabrica's return-type pointer in catalog: {catalog:#?}")
        });
    assert_eq!(return_type_pointer.shape, PointerShape::Scalar);
    assert!(!return_type_pointer.pointee_usr.is_empty());

    let local_pointer = catalog
        .iter()
        .find(|declaration| {
            declaration.kind == PointerDeclarationKind::Local && declaration.name == "origem"
        })
        .unwrap_or_else(|| panic!("expected origem local pointer in catalog: {catalog:#?}"));
    assert_eq!(
        local_pointer.pointee_usr, return_type_pointer.pointee_usr,
        "origem and fabrica's return type both point at Forma"
    );

    let project_store = syntax_bridge_server::persistence::ProjectStore::open(
        &project.project_dir.join("project.db"),
    )
    .expect("open project store");
    let persisted = project_store
        .list_pointer_declarations()
        .expect("list persisted pointer declarations");
    assert_eq!(persisted.len(), catalog.len());
    for declaration in catalog {
        assert!(
            persisted.contains(declaration),
            "expected persisted catalog to contain {declaration:?}"
        );
    }
}

const NARROWING_MAIN_CPP: &str = r#"
#include "fachada.hpp"

int main() {
    Forma* obtido = Obter();
    (void)obtido;
    return 0;
}
"#;

const NARROWING_FORMA_HPP: &str = r#"
class Forma {
public:
    virtual ~Forma() = default;
    virtual int Lados() const { return 0; }
};

class Triangulo : public Forma {
public:
    int Lados() const override { return 3; }
};

class Quadrado : public Forma {
public:
    int Lados() const override { return 4; }
};
"#;

const NARROWING_FABRICA_HPP: &str = r#"
#include "forma.hpp"

Forma *FabricaDeTriangulo();
"#;

const NARROWING_FABRICA_CPP: &str = r#"
#include "fabrica.hpp"

Forma *FabricaDeTriangulo() { return new Triangulo(); }
"#;

const NARROWING_FACHADA_HPP: &str = r#"
#include "forma.hpp"

Forma *Obter();
"#;

const NARROWING_FACHADA_CPP: &str = r#"
#include "fachada.hpp"
#include "fabrica.hpp"

Forma *Obter() { return FabricaDeTriangulo(); }
"#;

/// Proves the solver's narrowing (B07/B08, `docs/mapping-solver-cases.md`)
/// is reachable through the real product path — `project_service::
/// list_pointers` rebuilding `mapping::ProjectFacts` from what's already
/// persisted (no reparsing) and calling `mapping::pointer_options_for` with
/// each `return_type` pointer's own owning function — not just through the
/// `mapping-solver-fixtures/` corpus harness. `FabricaDeTriangulo` has
/// direct construction evidence (B07); `Obter` has none of its own and
/// narrows only by following the call graph to `FabricaDeTriangulo` (B08).
/// `Quadrado` exists in the project (declared in forma.hpp) but is never
/// constructed anywhere, so a correct answer of `{Triangulo}` for both
/// functions — not `{Forma, Triangulo, Quadrado}` — proves narrowing ran,
/// not just CHA.
#[test]
fn list_pointers_narrows_return_type_pointers_using_the_persisted_call_graph() {
    let workspace =
        TempWorkspace::new("pointer-catalog-narrowing").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_narrowing_fixture_tarball(workspace.path(), &archive_path)
        .expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "pointer_narrowing_fixture".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project and extract catalogs");

    let listing = project_service::list_pointers(&project.project_dir)
        .expect("list pointers from the persisted store");

    let fabrica_pointer = listing
        .pointers
        .iter()
        .find(|declaration| {
            declaration.kind == PointerDeclarationKind::ReturnType
                && declaration.name == "FabricaDeTriangulo"
        })
        .unwrap_or_else(|| {
            panic!(
                "expected FabricaDeTriangulo's return-type pointer: {:?}",
                listing.pointers
            )
        });
    let fabrica_types = listing
        .possible_types
        .get(&fabrica_pointer.usr)
        .unwrap_or_else(|| panic!("expected narrowed types for FabricaDeTriangulo"));
    assert_eq!(
        fabrica_types
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Triangulo"],
        "{fabrica_types:?}"
    );

    let obter_pointer = listing
        .pointers
        .iter()
        .find(|declaration| {
            declaration.kind == PointerDeclarationKind::ReturnType && declaration.name == "Obter"
        })
        .unwrap_or_else(|| {
            panic!(
                "expected Obter's return-type pointer: {:?}",
                listing.pointers
            )
        });
    let obter_types = listing
        .possible_types
        .get(&obter_pointer.usr)
        .unwrap_or_else(|| panic!("expected narrowed types for Obter"));
    assert_eq!(
        obter_types
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Triangulo"],
        "{obter_types:?} (should follow the call graph to FabricaDeTriangulo, not fall back to \
         the full CHA set {{Forma, Triangulo, Quadrado}})"
    );
}

fn write_narrowing_fixture_tarball(workspace: &Path, archive_path: &Path) -> io::Result<()> {
    let source_dir = workspace.join("fixture");
    fs::create_dir_all(&source_dir)?;
    fs::write(source_dir.join("main.cpp"), NARROWING_MAIN_CPP)?;
    fs::write(source_dir.join("forma.hpp"), NARROWING_FORMA_HPP)?;
    fs::write(source_dir.join("fabrica.hpp"), NARROWING_FABRICA_HPP)?;
    fs::write(source_dir.join("fabrica.cpp"), NARROWING_FABRICA_CPP)?;
    fs::write(source_dir.join("fachada.hpp"), NARROWING_FACHADA_HPP)?;
    fs::write(source_dir.join("fachada.cpp"), NARROWING_FACHADA_CPP)?;
    fs::write(
        source_dir.join("CMakeLists.txt"),
        r#"
cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_pointer_narrowing_fixture LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_executable(syntax_bridge_pointer_narrowing_fixture main.cpp fabrica.cpp fachada.cpp)
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
    fs::write(source_dir.join("main.cpp"), PROJECT_MAIN_CPP)?;
    fs::write(source_dir.join("types.h"), PROJECT_TYPES_H)?;
    fs::write(
        source_dir.join("CMakeLists.txt"),
        r#"
cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_pointer_catalog_fixture LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_executable(syntax_bridge_pointer_catalog_fixture main.cpp)
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
    path: std::path::PathBuf,
}

impl TempWorkspace {
    fn new(name: &str) -> std::io::Result<Self> {
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
