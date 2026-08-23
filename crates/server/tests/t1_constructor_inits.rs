use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use syntax_bridge_server::function_catalog;
use syntax_bridge_server::ingest::CompilationUnit;
use syntax_bridge_server::ir::{ConstructorInit, Expr, Stmt, Type};

fn write_fixture(project_root: &Path, source: &str, file_name: &str) -> CompilationUnit {
    fs::create_dir_all(project_root).expect("create project dir");
    let file_path = project_root.join(file_name);
    fs::write(&file_path, source).expect("write fixture source");
    CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    }
}

struct TempWorkspace {
    path: PathBuf,
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

fn lower_and_emit(name: &str, source: &str) -> String {
    let workspace = TempWorkspace::new(name).expect("create temporary workspace");
    fs::create_dir_all(workspace.path()).expect("create project dir");
    let file_path = workspace.path().join("probe.cpp");
    fs::write(&file_path, source).expect("write fixture source");
    let unit = CompilationUnit {
        directory: workspace.path().display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };
    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract function catalog");
    let module = syntax_bridge_server::ir::Module {
        functions: catalog.ir_functions.clone(),
        records: catalog.ir_records.clone(),
        enums: catalog.ir_enums.clone(),
    };
    syntax_bridge_server::emit::dart::emit_module(&module)
        .into_values()
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn constructor_field_inits_are_captured_in_ir_and_emitted_as_initializer_list() {
    let source = r#"
struct Ponto {
    int x;
    int y;
    Ponto(int a, int b) : x(a), y(b) {}
};
int usa() { Ponto p(3, 4); return p.x + p.y; }
"#;
    let workspace = TempWorkspace::new("t1-field-init").expect("create temp workspace");
    let unit = write_fixture(workspace.path(), source, "ponto.cpp");
    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract catalog");
    let ponto = catalog
        .ir_records
        .iter()
        .find(|r| r.name == "Ponto")
        .expect("Ponto record");
    assert_eq!(ponto.constructors.len(), 1, "expected one constructor");
    let ctor = &ponto.constructors[0];
    assert_eq!(ctor.inits.len(), 2, "expected two inits, got {:?}", ctor.inits);
    match &ctor.inits[0] {
        ConstructorInit::Field { name, value } => {
            assert_eq!(name, "x");
            match value {
                Expr::Ref { name: vname, .. } => assert_eq!(vname, "a"),
                _ => panic!("expected Ref a, got {:?}", value),
            }
        }
        other => panic!("expected Field x, got {:?}", other),
    }
    match &ctor.inits[1] {
        ConstructorInit::Field { name, value } => {
            assert_eq!(name, "y");
            match value {
                Expr::Ref { name: vname, .. } => assert_eq!(vname, "b"),
                _ => panic!("expected Ref b, got {:?}", value),
            }
        }
        other => panic!("expected Field y, got {:?}", other),
    }

    let dart = lower_and_emit("t1-field-emit", source);
    assert!(
        dart.contains(": x = a, y = b") || dart.contains(": x = a,y = b") || dart.contains("x = a, y = b"),
        "expected Dart initializer list ': x = a, y = b', got:\n{dart}"
    );
    assert!(
        !dart.contains("Point() {") || dart.contains("x = a"),
        "field inits should not be discarded, got:\n{dart}"
    );
}

#[test]
fn constructor_base_init_is_captured_and_emitted_as_super() {
    let source = r#"
struct Base {
    int v;
    Base(int v_) : v(v_) {}
};
struct Derivada : Base {
    int w;
    Derivada(int v, int w_) : Base(v), w(w_) {}
};
"#;
    let workspace = TempWorkspace::new("t1-base-init").expect("create temp workspace");
    let unit = write_fixture(workspace.path(), source, "base.cpp");
    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract catalog");
    let derivada = catalog
        .ir_records
        .iter()
        .find(|r| r.name == "Derivada")
        .expect("Derivada");
    assert_eq!(derivada.constructors.len(), 1);
    let ctor = &derivada.constructors[0];
    // Should have Base + Field
    assert_eq!(ctor.inits.len(), 2, "expected Base and Field, got {:?}", ctor.inits);
    let has_base = ctor.inits.iter().any(|init| matches!(init, ConstructorInit::Base { name, .. } if name == "Base"));
    assert!(has_base, "expected Base init, got {:?}", ctor.inits);
    assert!(
        ctor.inits.iter().any(|init| matches!(init, ConstructorInit::Field { name, .. } if name == "w")),
        "expected Field w"
    );

    let dart = lower_and_emit("t1-base-emit", source);
    assert!(
        dart.contains("super(v)") || dart.contains("super( v )"),
        "expected ': super(v)' in Dart, got:\n{dart}"
    );
    // super should be last
    if let Some(pos_super) = dart.find("super(") {
        if let Some(pos_w) = dart.find("w = w_") {
            assert!(pos_w < pos_super, "field init should come before super, got:\n{dart}");
        }
    }
}

#[test]
fn constructor_field_init_referencing_this_moves_to_body() {
    let source = r#"
struct Ponto {
    int x;
    int y;
    Ponto(int a) : x(a), y(x) {}
};
"#;
    let workspace = TempWorkspace::new("t1-this-ref").expect("create temp workspace");
    let unit = write_fixture(workspace.path(), source, "thisref.cpp");
    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract catalog");
    let ponto = catalog
        .ir_records
        .iter()
        .find(|r| r.name == "Ponto")
        .expect("Ponto");
    let ctor = &ponto.constructors[0];
    assert_eq!(ctor.inits.len(), 2);
    // Second init's value should contain This (field access)
    match &ctor.inits[1] {
        ConstructorInit::Field { name, value } => {
            assert_eq!(name, "y");
            // value should be FieldAccess with This
            let contains_this = format!("{:?}", value).contains("This");
            assert!(contains_this, "expected y(x) to reference this, got {:?}", value);
        }
        other => panic!("expected Field y, got {:?}", other),
    }

    let dart = lower_and_emit("t1-this-emit", source);
    // x = a should be in initializer list, y should be in body
    assert!(
        dart.contains("x = a"),
        "x = a should be in initializer list, got:\n{dart}"
    );
    // y = x should be in body, not initializer list (or at least not as 'y = x' in initializer with this)
    // Check that body contains y assignment
    // The Dart should have ": x = a {" and then "y = x;" inside
    assert!(
        dart.contains("y = x") || dart.contains("y = this.x") || dart.contains("y = x;"),
        "y referencing this should be moved to body as 'y = x;', got:\n{dart}"
    );
    // Ensure initializer does not contain y = x with this reference in the initializer part before '{'
    // We check that after ':' and before '{', there is no 'y ='
    if let Some(colon) = dart.find(':') {
        if let Some(brace) = dart[colon..].find('{') {
            let init_part = &dart[colon..colon+brace];
            // init_part should contain x but not y (since y moved)
            assert!(
                init_part.contains("x = a") && !init_part.contains("y ="),
                "y init should not be in initializer list, init_part={init_part}, full:\n{dart}"
            );
        }
    }
}
