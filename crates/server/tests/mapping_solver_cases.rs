//! Harness for `mapping-solver-fixtures/` (see `docs/mapping-solver-cases.md`):
//! exercises the US-7 solver in `crates/server/src/mapping.rs` —
//! `options_for`, `overload_options_for`, `template_options_for`,
//! `signature_options_for`, `string_usage_conflict` — against real
//! `libclang`-extracted catalogs of the 22 documented cases, instead of
//! hand-built `TypeDeclaration`s/`FunctionDeclaration`s. Each `#[test]`
//! below is one case from the document; the assertions are what
//! `docs/mapping-solver-cases.md` documents as that case's correct
//! decision.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use syntax_bridge_server::function_catalog::{self, FunctionCatalog, FunctionDeclaration};
use syntax_bridge_server::ingest;
use syntax_bridge_server::mapping::{self, ProjectFacts};
use syntax_bridge_server::type_catalog::{self, TypeCatalog, TypeDeclaration};

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("mapping-solver-fixtures")
}

fn extract(case: &str) -> (TypeCatalog, FunctionCatalog) {
    let case_dir = fixtures_root().join(case);
    let workspace = TempWorkspace::new(&format!("mapping-case-{case}")).expect("temp workspace");
    let build_dir = workspace.path().join("build");

    run_command(
        Command::new("cmake")
            .arg("-S")
            .arg(&case_dir)
            .arg("-B")
            .arg(&build_dir)
            .arg("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON"),
    )
    .expect("cmake configure");

    let compilation_units =
        ingest::read_compilation_units(&build_dir.join("compile_commands.json"))
            .expect("read compile_commands.json");
    let project_root = case_dir.canonicalize().unwrap_or(case_dir);

    let types = type_catalog::extract_type_catalog(&compilation_units, &project_root, None)
        .expect("extract type catalog");
    let functions =
        function_catalog::extract_function_catalog(&compilation_units, &project_root, None)
            .expect("extract function catalog");

    (types, functions)
}

fn find_type<'a>(catalog: &'a TypeCatalog, name: &str) -> &'a TypeDeclaration {
    catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == name)
        .unwrap_or_else(|| panic!("no type named {name} in catalog"))
}

fn find_function<'a>(catalog: &'a FunctionCatalog, name: &str) -> &'a FunctionDeclaration {
    catalog
        .declarations
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("no function named {name} in catalog"))
}

fn find_function_where<'a>(
    catalog: &'a FunctionCatalog,
    name: &str,
    predicate: impl Fn(&FunctionDeclaration) -> bool,
) -> &'a FunctionDeclaration {
    catalog
        .declarations
        .iter()
        .find(|function| function.name == name && predicate(function))
        .unwrap_or_else(|| panic!("no function named {name} matching predicate in catalog"))
}

// ---------------------------------------------------------------------
// Category A — local: the answer is in the file itself
// ---------------------------------------------------------------------

#[test]
fn a01_multiple_inheritance_without_conflict_gets_one_mixin_option() {
    let (types, functions) = extract("A01-heranca-multipla-sem-conflito");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let pato = find_type(&types, "Pato");

    let options = mapping::options_for(pato, &facts, &[]);
    assert_eq!(options.len(), 1, "{options:?}");
    assert_eq!(options[0].id, "classe-com-mixins");
    assert_eq!(options[0].consequences.len(), 2);
    let bases: Vec<&str> = types
        .declarations
        .iter()
        .filter(|declaration| ["Voador", "Nadador"].contains(&declaration.name.as_str()))
        .map(|declaration| declaration.usr.as_str())
        .collect();
    for consequence in &options[0].consequences {
        assert!(
            bases.contains(&consequence.affected_type_usr.as_str()),
            "{consequence:?}"
        );
    }

    // Bonus: Voador/Nadador are themselves pure-interface candidates (one
    // pure virtual method each, no fields, no bases) — B03's rule exercised
    // on the "obviously fine" side.
    for interface_name in ["Voador", "Nadador"] {
        let interface = find_type(&types, interface_name);
        let interface_options = mapping::options_for(interface, &facts, &[]);
        assert_eq!(interface_options.len(), 1);
        assert_eq!(
            interface_options[0].id, "interface-pura",
            "{interface_name}"
        );
    }
}

#[test]
fn a02_overload_by_arity_gets_optional_parameter_by_type_gets_renamed() {
    let (_types, functions) = extract("A02-sobrecarga-aridade-e-tipo");
    let facts = ProjectFacts::from_catalogs(&_types, &functions);

    let area_options = mapping::overload_options_for(None, "area", &facts);
    assert_eq!(area_options.len(), 1);
    assert_eq!(area_options[0].id, "parametro-opcional");

    let para_texto_options = mapping::overload_options_for(None, "paraTexto", &facts);
    assert_eq!(para_texto_options.len(), 1);
    assert_eq!(para_texto_options[0].id, "renomear-por-tipo");
    assert_eq!(para_texto_options[0].consequences.len(), 2);
}

#[test]
fn a03_union_always_gets_a_bridge_option() {
    let (types, functions) = extract("A03-uniao-simples");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let valor_numerico = find_type(&types, "ValorNumerico");

    let options = mapping::options_for(valor_numerico, &facts, &[]);
    assert_eq!(options.len(), 1, "{options:?}");
    assert_eq!(options[0].id, "uniao-com-tag");
}

#[test]
fn a04_fixed_width_integer_and_pointer_both_flagged() {
    let (_types, functions) = extract("A04-inteiros-largura-fixa-overflow");
    let checksum = find_function(&functions, "checksum");

    let options = mapping::signature_options_for(checksum);
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].id, "codigo-ponte");
    assert!(
        options[0].description.contains("uint8_t"),
        "{:?}",
        options[0]
    );
    assert!(
        options[0].description.contains("ponteiro"),
        "{:?}",
        options[0]
    );
}

#[test]
fn a05_const_vs_non_const_overload_needs_renaming() {
    let (types, functions) = extract("A05-const-e-nao-const-overload");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let contador = find_type(&types, "Contador");

    let options = mapping::overload_options_for(Some(contador.usr.as_str()), "valor", &facts);
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].id, "renomear-const-nao-const");
    assert_eq!(options[0].consequences.len(), 2);
}

#[test]
fn a06_template_instantiated_in_one_file_is_local() {
    let (_types, functions) = extract("A06-template-monomorfizacao-local");
    let facts = ProjectFacts::from_catalogs(&_types, &functions);
    let maior = find_function(&functions, "maior");

    let options = mapping::template_options_for(maior, &facts);
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].id, "monomorfizacao-local");
}

#[test]
fn a07_direct_operator_overload_maps_to_dart_operator() {
    let (types, functions) = extract("A07-operador-sobrecarregado-direto");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let vetor2 = find_type(&types, "Vetor2");

    for operator_name in ["operator+", "operator=="] {
        let options =
            mapping::overload_options_for(Some(vetor2.usr.as_str()), operator_name, &facts);
        assert_eq!(options.len(), 1, "{operator_name}");
        assert_eq!(options[0].id, "operador-direto", "{operator_name}");
    }
}

#[test]
fn a08_float_flagged_double_is_not() {
    let (_types, functions) = extract("A08-float-vs-double-precisao");
    let dividir_float = find_function(&functions, "dividirFloat");
    let dividir_double = find_function(&functions, "dividirDouble");

    let float_options = mapping::signature_options_for(dividir_float);
    assert_eq!(float_options[0].id, "codigo-ponte");
    assert!(float_options[0].description.contains("float"));

    let double_options = mapping::signature_options_for(dividir_double);
    assert_eq!(double_options[0].id, "assinatura-direta");
}

#[test]
fn a09_trivial_stl_vector_needs_no_bridge() {
    let (_types, functions) = extract("A09-vetor-stl-trivial");
    let dobrar = find_function(&functions, "dobrar");

    let options = mapping::signature_options_for(dobrar);
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].id, "assinatura-direta");
}

// ---------------------------------------------------------------------
// Category B — global: the answer only appears combining files
// ---------------------------------------------------------------------

#[test]
fn b01_cross_file_mutation_forces_the_class_to_stay_mutable() {
    let (types, functions) = extract("B01-duas-classes-com-restricao-cruzada");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let ponto3d = find_type(&types, "Ponto3D");

    // Still exactly one option (criterion 1 never offers alternatives) —
    // what's new is *why*: the option's own consequence must cite the
    // cross-file mutator, proving the check ran against the whole project,
    // not just ponto3d.hpp.
    let options = mapping::options_for(ponto3d, &facts, &[]);
    assert_eq!(options.len(), 1, "{options:?}");
    assert_eq!(options[0].id, "classe-direta");
    assert_eq!(options[0].consequences.len(), 1, "{options:?}");
    assert!(
        options[0].consequences[0]
            .description
            .contains("AtualizadorDePosicao"),
        "{:?}",
        options[0]
    );

    // Negative control: with only Ponto3D's own file in the catalog (no
    // AtualizadorDePosicao in sight), the same option comes back with no
    // justification — the cross-file fact genuinely drove the difference
    // above, not some property of Ponto3D alone.
    let isolated_declarations = vec![ponto3d.clone()];
    let isolated_facts = ProjectFacts::new(&isolated_declarations);
    let isolated_options = mapping::options_for(ponto3d, &isolated_facts, &[]);
    assert_eq!(isolated_options[0].id, "classe-direta");
    assert!(
        isolated_options[0].consequences.is_empty(),
        "{isolated_options:?}"
    );
}

#[test]
fn b02_diamond_with_conflicting_methods_needs_explicit_override() {
    let (types, functions) = extract("B02-diamante-com-metodos-conflitantes");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let combinado = find_type(&types, "Combinado");

    let options = mapping::options_for(combinado, &facts, &[]);
    assert_eq!(options.len(), 1, "{options:?}");
    assert_eq!(options[0].id, "mixins-com-sobrescrita-explicita");
    assert!(
        options[0]
            .consequences
            .iter()
            .any(|consequence| consequence.affected_type_usr == combinado.usr
                && consequence.description.contains("nome")),
        "{:?}",
        options[0]
    );
}

#[test]
fn b03_interface_with_one_non_pure_method_becomes_a_mixin() {
    let (types, functions) = extract("B03-interface-implementada-em-varios-locais");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let desenhavel = find_type(&types, "Desenhavel");

    let options = mapping::options_for(desenhavel, &facts, &[]);
    assert_eq!(options.len(), 1, "{options:?}");
    assert_eq!(options[0].id, "mixin-com-implementacao-padrao");
    assert!(
        options[0]
            .consequences
            .iter()
            .any(|consequence| consequence.description.contains("descricaoPadrao")),
        "{:?}",
        options[0]
    );
}

#[test]
fn b04_overload_rename_propagates_to_a_caller_in_a_fourth_file() {
    let (types, functions) = extract("B04-sobrecarga-entre-unidades-de-compilacao");
    let facts = ProjectFacts::from_catalogs(&types, &functions);

    let options = mapping::overload_options_for(None, "formatar", &facts);
    assert_eq!(options.len(), 1, "{options:?}");
    assert_eq!(options[0].id, "renomear-por-tipo");
    assert!(
        options[0].consequences.iter().any(|consequence| consequence
            .description
            .contains("gerarRelatorio")
            && consequence.description.contains("relatorio.cpp")),
        "expected a call-site consequence naming gerarRelatorio in relatorio.cpp, got {:?}",
        options[0].consequences
    );
}

#[test]
fn b05_string_used_as_text_and_binary_is_a_product_decision() {
    let (types, functions) = extract("B05-string-texto-em-um-lugar-binario-em-outro");
    let facts = ProjectFacts::from_catalogs(&types, &functions);

    let option = mapping::string_usage_conflict(&facts).expect("expected a conflict to be found");
    assert_eq!(option.id, "decisao-de-produto-string-vs-bytes");
    assert_eq!(option.consequences.len(), 2);
}

#[test]
fn b06_virtual_inheritance_diamond_gets_mixin_composition() {
    let (types, functions) = extract("B06-heranca-virtual-estado-compartilhado");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let anfibio = find_type(&types, "Anfibio");

    // `andar`/`remar` don't collide by name, so this lands on the
    // conflict-free multiple-inheritance branch (composition via mixins) —
    // the correct high-level answer. What this rule set does *not* yet
    // capture is the deeper reason B06 exists: that VeiculoTerrestre and
    // Barco share a single Motor subobject (C++ virtual inheritance), which
    // `type_catalog` doesn't record at all (no "is this base virtual?"
    // flag) — see docs/mapping-solver-cases.md's B06 writeup. Composition
    // must preserve that shared identity; this test only proves composition
    // (not flattening) was chosen, not that identity-sharing was modeled.
    let options = mapping::options_for(anfibio, &facts, &[]);
    assert_eq!(options.len(), 1, "{options:?}");
    assert_eq!(options[0].id, "classe-com-mixins");
    assert_eq!(options[0].consequences.len(), 2);
}

/// B07: `possible_pointee_types` today is class-hierarchy analysis (CHA) —
/// it enumerates every subclass of the pointee's declared type, regardless
/// of what a *specific* pointer is ever actually constructed as. `Forma`
/// has two subclasses in this fixture (`Triangulo`, `Quadrado`), both
/// declared in `fabrica.cpp` — CHA alone would offer `{Forma, Triangulo,
/// Quadrado}` for *either* function's return pointer, even though each
/// function's own body shows it only ever constructs one of them. This is
/// the first case in the corpus that needs to go beyond CHA (see
/// `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`).
#[test]
fn b07_pointer_with_a_single_construction_site_narrows_past_the_full_hierarchy() {
    let (types, functions) = extract("B07-ponteiro-com-atribuicao-unica");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let forma = find_type(&types, "Forma");
    let pointee = mapping::PointeeShape::Known {
        usr: forma.usr.clone(),
        name: forma.name.clone(),
    };

    // Without an owning function to narrow against, the answer stays the
    // full, sound CHA enumeration — narrowing is additive, never silently
    // on by default.
    let unnarrowed = mapping::pointer_options_for(pointee.clone(), Some(&facts), None);
    let unnarrowed_names: Vec<&str> = unnarrowed[0]
        .consequences
        .iter()
        .map(|consequence| consequence.description.as_str())
        .collect();
    assert_eq!(
        unnarrowed[0].consequences.len(),
        3,
        "{unnarrowed:?} (expected Forma, Triangulo and Quadrado, unnarrowed)"
    );
    let _ = unnarrowed_names;

    let fabrica_de_triangulo = find_function(&functions, "FabricaDeTriangulo");
    let narrowed_to_triangulo =
        mapping::pointer_options_for(pointee.clone(), Some(&facts), Some(fabrica_de_triangulo));
    assert_eq!(
        narrowed_to_triangulo[0].consequences.len(),
        1,
        "{narrowed_to_triangulo:?}"
    );
    assert!(
        narrowed_to_triangulo[0].consequences[0]
            .description
            .contains("Triangulo"),
        "{narrowed_to_triangulo:?}"
    );

    let fabrica_de_quadrado = find_function(&functions, "FabricaDeQuadrado");
    let narrowed_to_quadrado =
        mapping::pointer_options_for(pointee, Some(&facts), Some(fabrica_de_quadrado));
    assert_eq!(
        narrowed_to_quadrado[0].consequences.len(),
        1,
        "{narrowed_to_quadrado:?}"
    );
    assert!(
        narrowed_to_quadrado[0].consequences[0]
            .description
            .contains("Quadrado"),
        "{narrowed_to_quadrado:?}"
    );
}

// ---------------------------------------------------------------------
// Category C — bridge code is the only viable path
// ---------------------------------------------------------------------

#[test]
fn c01_pointer_arithmetic_needs_dart_ffi() {
    let (_types, functions) = extract("C01-aritmetica-de-ponteiros");
    let soma_janela = find_function(&functions, "somaJanela");

    let options = mapping::signature_options_for(soma_janela);
    assert_eq!(options[0].id, "codigo-ponte");
    assert!(
        options[0].description.contains("dart:ffi"),
        "{:?}",
        options[0]
    );
}

#[test]
fn c02_setjmp_longjmp_has_no_dart_equivalent() {
    let (_types, functions) = extract("C02-setjmp-longjmp");
    let protegido = find_function(&functions, "protegido");
    let arriscado = find_function(&functions, "arriscado");

    let protegido_options = mapping::signature_options_for(protegido);
    assert_eq!(protegido_options[0].id, "codigo-ponte");
    assert!(
        protegido_options[0].description.contains("setjmp"),
        "{:?}",
        protegido_options[0]
    );

    let arriscado_options = mapping::signature_options_for(arriscado);
    assert_eq!(arriscado_options[0].id, "codigo-ponte");
}

#[test]
fn c03_conditional_compilation_is_a_product_decision_not_a_type_mapping() {
    let (types, functions) = extract("C03-compilacao-condicional");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let config = find_type(&types, "Config");

    let options = mapping::options_for(config, &facts, &[]);
    assert_eq!(options.len(), 1, "{options:?}");
    assert_eq!(options[0].id, "decisao-de-produto-compilacao-condicional");
}

#[test]
fn c04_shared_memory_threading_needs_isolate_rewrite() {
    let (_types, functions) = extract("C04-threads-e-mutex");
    let incrementar = find_function(&functions, "incrementarEmParalelo");
    let valor = find_function_where(&functions, "valor", |function| {
        function.signature.contains("const")
    });

    let incrementar_options = mapping::signature_options_for(incrementar);
    assert_eq!(incrementar_options[0].id, "codigo-ponte");
    assert!(
        incrementar_options[0].description.contains("thread"),
        "{:?}",
        incrementar_options[0]
    );

    let valor_options = mapping::signature_options_for(valor);
    assert_eq!(valor_options[0].id, "assinatura-direta");
}

#[test]
fn c05_rule_of_three_needs_explicit_cloning() {
    let (types, functions) = extract("C05-semantica-de-valor-com-ponteiro-proprio");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let buffer_proprio = find_type(&types, "BufferProprio");

    let options = mapping::options_for(buffer_proprio, &facts, &[]);
    assert_eq!(options.len(), 1, "{options:?}");
    assert_eq!(options[0].id, "clonagem-explicita-valor");
}

#[test]
fn c06_raii_over_external_resource_needs_explicit_dispose() {
    let (types, functions) = extract("C06-raii-recurso-externo");
    let facts = ProjectFacts::from_catalogs(&types, &functions);
    let arquivo_texto = find_type(&types, "ArquivoTexto");

    let options = mapping::options_for(arquivo_texto, &facts, &[]);
    assert_eq!(options.len(), 1, "{options:?}");
    assert_eq!(options[0].id, "dispose-explicito-raii");
}

#[test]
fn c07_goto_shared_cleanup_has_no_dart_equivalent() {
    let (_types, functions) = extract("C07-goto-limpeza-compartilhada");
    let processar = find_function(&functions, "processarComDoisRecursos");

    let options = mapping::signature_options_for(processar);
    assert_eq!(options[0].id, "codigo-ponte");
    assert!(options[0].description.contains("goto"), "{:?}", options[0]);
}

// ---------------------------------------------------------------------
// Test plumbing
// ---------------------------------------------------------------------

fn run_command(command: &mut Command) -> Result<Output, String> {
    let output = command
        .output()
        .map_err(|error| format!("failed to spawn {command:?}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{command:?} exited with {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output)
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
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
