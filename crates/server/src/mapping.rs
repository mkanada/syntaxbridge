//! US-7 (mapeamento de tipos C++ → Dart): decide, para cada tipo e cada
//! grupo de sobrecargas de um projeto, quais mapeamentos para Dart são
//! oferecidos — nunca uma lista vazia (Q9), e nunca uma opção que
//! inviabilize outro tipo do projeto (critério 3, quando detectável).
//!
//! O corpus de `mapping-solver-fixtures/` (documentado em
//! `docs/mapping-solver-cases.md`) é o que dirige as regras abaixo — cada
//! regra existe porque um caso concreto do corpus a exige, testado em
//! `crates/server/tests/mapping_solver_cases.rs` contra catálogos extraídos
//! de verdade (não `TypeDeclaration`s escritos à mão). Este não é o solver
//! de satisfação de restrições completo que o roteiro de US-7 descreve como
//! "o item mais caro" — é um primeiro corte baseado em regras sobre fatos já
//! extraídos por `type_catalog`/`function_catalog` (base classes via
//! `TypeUsage::Inheritance`, sobrecargas/`const`/virtualidade via
//! `FunctionDeclaration::signature`) mais, para o que nenhum dos dois
//! catálogos expõe (ponteiros, `std::thread`/`std::mutex`, `setjmp`/`goto`,
//! `#ifdef`), uma varredura textual da assinatura/corpo/arquivo fonte —
//! sempre documentada como heurística, nunca apresentada como certeza.

use std::collections::HashSet;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::function_catalog::{
    CallEdge, CallResolution, FunctionCatalog, FunctionDeclaration, FunctionDeclarationKind,
};
use crate::type_catalog::{
    TypeCatalog, TypeDeclaration, TypeDeclarationKind, TypeUsage, TypeUsageKind,
};

/// What choosing an option changes about another type — e.g. "escolher
/// mixin para `Base` obriga `Derived` a virar `with Base`". Structured (not
/// free text) so criterion 3 of US-7 ("uma opção que tornaria outro tipo não
/// convertível não é oferecida") is machine-checkable once a real solver
/// exists.
///
/// Reused across the three solver entry points below (`options_for`,
/// `overload_options_for`, `template_options_for`) — `affected_type_usr` is
/// whichever declaration's `usr` is affected, a type's or a function's; the
/// field predates the function-level entry points and keeping one name
/// avoids a parallel `affected_function_usr` for what is, structurally, the
/// same fact ("this other declaration is affected, here's how").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Consequence {
    pub affected_type_usr: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MappingOption {
    pub id: String,
    pub label: String,
    pub description: String,
    pub consequences: Vec<Consequence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MappingDecision {
    pub type_usr: String,
    pub option_id: String,
    /// RFC 3339 timestamp — a plain `String` (like the rest of this slice)
    /// rather than a dedicated time type, since nothing here parses or
    /// compares it; it is only ever stored and displayed.
    pub decided_at: String,
}

/// Every fact the solver draws on, gathered project-wide — the "catalog and
/// decisions" `options_for` always accepted, widened to what the corpus in
/// `docs/mapping-solver-cases.md` actually needs: base-class edges,
/// signatures/virtuality of every function, and the call graph (for
/// cross-file consequence propagation, e.g. case B04).
///
/// A degraded `ProjectFacts` (empty `usages`/`functions`/`calls`, see
/// [`ProjectFacts::new`]) is what every pre-existing caller gets — E01–E03
/// fixtures never have more than one base class or one method, so nothing
/// the rules below check ever fires for them, and behavior is unchanged
/// from before this module grew these fields.
#[derive(Debug, Clone, Copy)]
pub struct ProjectFacts<'a> {
    pub declarations: &'a [TypeDeclaration],
    pub usages: &'a [TypeUsage],
    pub functions: &'a [FunctionDeclaration],
    pub calls: &'a [CallEdge],
}

impl<'a> ProjectFacts<'a> {
    /// A degraded context with only the type catalog's declarations — what
    /// `transpile::emit_package` builds today, since it only ever has a flat
    /// `&[TypeDeclaration]` in hand (see that module's doc comment on
    /// `options_for`'s call site).
    pub fn new(declarations: &'a [TypeDeclaration]) -> Self {
        Self {
            declarations,
            usages: &[],
            functions: &[],
            calls: &[],
        }
    }

    pub fn from_catalogs(types: &'a TypeCatalog, functions: &'a FunctionCatalog) -> Self {
        Self {
            declarations: &types.declarations,
            usages: &types.usages,
            functions: &functions.declarations,
            calls: &functions.calls,
        }
    }
}

// ---------------------------------------------------------------------
// Type-level decisions
// ---------------------------------------------------------------------

/// The viable mapping options for `declaration`, given every other fact
/// `facts` carries about the project. Never returns an empty `Vec`
/// (criterion 5 of US-7 / Q9): a type with no direct mapping still gets a
/// bridge-code option, with a reason.
pub fn options_for(
    declaration: &TypeDeclaration,
    facts: &ProjectFacts<'_>,
    _decisions: &[MappingDecision],
) -> Vec<MappingOption> {
    if !matches!(
        declaration.kind,
        TypeDeclarationKind::Struct | TypeDeclarationKind::Class
    ) {
        return non_record_options(declaration);
    }

    // Checked before anything else: `libclang` only ever sees the branch a
    // single `compile_commands.json` compiled, so no rule below this line
    // can see the *other* branch of a `#ifdef` — the correct move is to say
    // so, not to quietly analyze one branch as if it were the whole type
    // (case C03, `docs/mapping-solver-cases.md`).
    if has_conditional_compilation(&declaration.file) {
        return vec![MappingOption {
            id: "decisao-de-produto-compilacao-condicional".to_owned(),
            label: "Decisão de produto: compilação condicional".to_owned(),
            description: format!(
                "`{}` está em um arquivo com compilação condicional (`#ifdef`/`#ifndef`/`#if`). \
                 `libclang` só enxerga o ramo que foi de fato compilado nesta configuração — o \
                 outro ramo é texto morto para a análise, mesmo que o produto real precise \
                 converter para as duas configurações. Não há opção de mapeamento de tipo que \
                 resolva isto: é uma decisão de produto (gerar as duas variantes atrás de uma \
                 flag, ou perguntar ao usuário qual configuração converter), não uma escolha \
                 entre representações Dart.",
                declaration.name
            ),
            consequences: Vec::new(),
        }];
    }

    let bases = base_usrs_of(declaration, facts);

    if bases.len() >= 2 {
        return vec![multiple_inheritance_option(declaration, facts, &bases)];
    }

    if let Some(option) = value_semantics_option(declaration, facts) {
        return vec![option];
    }

    if bases.is_empty()
        && let Some(option) = interface_candidate_option(declaration, facts)
    {
        return vec![option];
    }

    vec![default_class_option(declaration, facts)]
}

/// The first real slice of the project-wide viability solver Q9 promised
/// (US-7 roteiro item 4, `docs/plans/User Steps.md`) — not the full
/// constraint-satisfaction solver the roteiro calls "o item mais caro" of
/// US-7 (that stays gated on E09, per the same document), but a genuine
/// first check that goes beyond `options_for`'s own file-scoped view.
///
/// `options_for(declaration, ...)` only ever reasons about `declaration`
/// and its own direct bases; it has no way to know that some *other*
/// type's own option already forces a shape onto `declaration` — today,
/// concretely, `multiple_inheritance_option`'s `"classe-com-mixins"`
/// option, which attaches a `Consequence` to every base saying it "vira
/// mixin" (case B09, `docs/mapping-solver-cases.md`). A Dart `mixin` can
/// never be instantiated on its own; when `declaration` is *also* directly
/// constructed as a plain value somewhere else in the project
/// (`TypeUsageKind::VariableDeclaration`), the two requirements can't both
/// hold — `options_for`'s own answer for `declaration`, whatever it is, is
/// not actually feasible. `feasible_options` re-derives that "some other
/// type forces a mixin here" fact by calling `options_for` on every other
/// declaration (rather than duplicating `multiple_inheritance_option`'s own
/// matching logic), so the two never drift apart, and only overrides the
/// default when both halves of the conflict hold — criterion 3 ("uma opção
/// que tornaria outro tipo não convertível não é oferecida") enforced for
/// real, for this one shape of conflict, not just described.
pub fn feasible_options(
    declaration: &TypeDeclaration,
    facts: &ProjectFacts<'_>,
    decisions: &[MappingDecision],
) -> Vec<MappingOption> {
    let default = options_for(declaration, facts, decisions);

    let Some(forcing) = mixin_forced_on(declaration, facts) else {
        return default;
    };
    if !is_directly_value_constructed(declaration, facts) {
        return default;
    }

    vec![MappingOption {
        id: "ponte-mixin-inviavel".to_owned(),
        label: "Código ponte: mixin inviável".to_owned(),
        description: format!(
            "`{name}` precisaria virar `mixin` para satisfazer a composição de herança \
             múltipla de `{forcing_name}` — mas `{name}` também é instanciado diretamente \
             como valor em algum outro lugar do projeto, e um `mixin` em Dart nunca pode ser \
             instanciado sozinho. As duas exigências não são simultaneamente satisfazíveis por \
             uma única classe/mixin: precisa de código ponte (composição em vez de mixin, ou \
             duas representações).",
            name = declaration.name,
            forcing_name = forcing.name
        ),
        consequences: vec![Consequence {
            affected_type_usr: forcing.usr,
            description: format!(
                "`{}` não pode mais compor `{}` via `with` — `{}` precisa continuar \
                 instanciável diretamente em algum outro lugar do projeto",
                forcing.name, declaration.name, declaration.name
            ),
        }],
    }]
}

/// The type (if any) whose own `options_for` result already forces
/// `declaration` to become a Dart `mixin` — found by asking every other
/// declaration what *it* would choose (reusing `options_for` itself, not a
/// parallel copy of `multiple_inheritance_option`'s own matching logic) and
/// checking whether that answer's own consequences name `declaration`.
struct ForcingType {
    usr: String,
    name: String,
}

fn mixin_forced_on(declaration: &TypeDeclaration, facts: &ProjectFacts<'_>) -> Option<ForcingType> {
    facts.declarations.iter().find_map(|other| {
        if other.usr == declaration.usr {
            return None;
        }

        let forces = options_for(other, facts, &[]).iter().any(|option| {
            option.consequences.iter().any(|consequence| {
                consequence.affected_type_usr == declaration.usr
                    && consequence.description.contains("vira mixin")
            })
        });

        forces.then(|| ForcingType {
            usr: other.usr.clone(),
            name: other.name.clone(),
        })
    })
}

/// Whether `declaration` is used as a file/namespace-scope variable's own
/// type anywhere in the project — `TypeUsageKind::VariableDeclaration`,
/// already extracted by `type_catalog` (US-4's usage taxonomy), reused
/// here rather than a new textual scan.
fn is_directly_value_constructed(declaration: &TypeDeclaration, facts: &ProjectFacts<'_>) -> bool {
    facts.usages.iter().any(|usage| {
        usage.type_usr == declaration.usr && usage.kind == TypeUsageKind::VariableDeclaration
    })
}

/// Kinds `options_for` doesn't have a dedicated rule for yet: `union` always
/// needs bridge code (Dart has no overlapping-memory type); everything else
/// (enum, typedef, type alias, macros) falls back to the original US-7
/// slice's bridge-code placeholder, unchanged from before this module grew
/// real rules.
fn non_record_options(declaration: &TypeDeclaration) -> Vec<MappingOption> {
    if declaration.kind == TypeDeclarationKind::Union {
        return vec![MappingOption {
            id: "uniao-com-tag".to_owned(),
            label: "Código ponte: classe com tag".to_owned(),
            description: format!(
                "`{}` é `union`: os campos nunca compartilham memória em Dart. Código ponte \
                 necessário — uma classe com uma tag mais um campo por alternativa, ou um \
                 wrapper sobre bytes crus se a sobreposição binária em si importar.",
                declaration.name
            ),
            consequences: Vec::new(),
        }];
    }

    vec![MappingOption {
        id: "codigo-ponte".to_owned(),
        label: "Código ponte".to_owned(),
        description: format!(
            "Nenhum mapeamento direto existe ainda para `{}` ({:?}) — código ponte \
             mantém a conversão possível até que um mapeamento dedicado seja \
             implementado.",
            declaration.name, declaration.kind
        ),
        consequences: Vec::new(),
    }]
}

/// Criterion 2 of US-7: a class with multiple inheritance gets at least one
/// class+mixin combination, with consequences. Still exactly one option
/// (criterion 1's "sem apresentar alternativas" is written for the
/// no-multiple-inheritance case, but nothing in the corpus needs a *second*
/// option here yet — both A01 and B02 are settled by which single option is
/// safe to offer, not by choosing among several).
fn multiple_inheritance_option(
    declaration: &TypeDeclaration,
    facts: &ProjectFacts<'_>,
    bases: &[String],
) -> MappingOption {
    let base_name = |usr: &str| -> String {
        facts
            .declarations
            .iter()
            .find(|decl| decl.usr == usr)
            .map(|decl| decl.name.clone())
            .unwrap_or_else(|| usr.to_owned())
    };

    if let Some(conflict) = conflicting_diamond_override(declaration, facts, bases) {
        let conflicting_bases: Vec<&String> = conflict
            .overridden_usrs
            .iter()
            .filter(|usr| bases.contains(usr))
            .collect();
        let mut consequences: Vec<Consequence> = bases
            .iter()
            .map(|base| Consequence {
                affected_type_usr: base.clone(),
                description: format!(
                    "`{}` vira mixin aplicado via `with` em `{}`",
                    base_name(base),
                    declaration.name
                ),
            })
            .collect();
        consequences.push(Consequence {
            affected_type_usr: declaration.usr.clone(),
            description: format!(
                "`{}` precisa sobrescrever `{}` explicitamente — {} declaram o método de forma \
                 incompatível, e `with` sozinho (que só usa a última declaração da lista) \
                 produziria um resultado diferente do C++ original",
                declaration.name,
                conflict.name,
                conflicting_bases
                    .iter()
                    .map(|usr| format!("`{}`", base_name(usr)))
                    .collect::<Vec<_>>()
                    .join(" e ")
            ),
        });

        return MappingOption {
            id: "mixins-com-sobrescrita-explicita".to_owned(),
            label: "Classe com mixins e sobrescrita explícita".to_owned(),
            description: format!(
                "`{}` combina {} bases via herança múltipla; pelo menos um método é declarado \
                 de forma conflitante entre elas, então a classe Dart precisa sobrescrever esse \
                 método explicitamente em vez de confiar na ordem de `with`.",
                declaration.name,
                bases.len()
            ),
            consequences,
        };
    }

    let consequences = bases
        .iter()
        .map(|base| Consequence {
            affected_type_usr: base.clone(),
            description: format!(
                "`{}` vira mixin aplicado via `with` em `{}`",
                base_name(base),
                declaration.name
            ),
        })
        .collect();

    MappingOption {
        id: "classe-com-mixins".to_owned(),
        label: "Classe com mixins".to_owned(),
        description: format!(
            "`{}` combina {} bases via herança múltipla, sem conflito de nomes entre elas: cada \
             base vira um mixin aplicado via `with`.",
            declaration.name,
            bases.len()
        ),
        consequences,
    }
}

/// A method whose `overridden_usrs` reaches into two or more of `bases` —
/// i.e., the derived type had to resolve an ambiguity C++ itself would
/// otherwise reject (case B02: `Combinado::nome()` overrides both
/// `BaseA::nome()` and `BaseB::nome()`).
fn conflicting_diamond_override<'a>(
    declaration: &TypeDeclaration,
    facts: &'a ProjectFacts<'_>,
    bases: &[String],
) -> Option<&'a FunctionDeclaration> {
    methods_of(declaration, facts).into_iter().find(|method| {
        let owners_among_bases: HashSet<&str> = method
            .overridden_usrs
            .iter()
            .filter_map(|usr| facts.functions.iter().find(|f| &f.usr == usr))
            .filter_map(|f| f.owning_class_usr.as_deref())
            .filter(|owner| bases.iter().any(|base| base == owner))
            .collect();
        owners_among_bases.len() >= 2
    })
}

/// Rule of Three (case C05) or plain RAII (case C06): a user-declared
/// destructor is the signal `function_catalog` gives us that C++ semantics
/// here aren't "an implicit, do-nothing teardown" — no implicit destructor
/// is ever a cursor in the AST, so its presence in the catalog already means
/// someone wrote one on purpose. A *defaulted* one (`= default`, e.g.
/// `virtual ~Voador() = default;` written only to give a base class a safe
/// polymorphic destructor — case A01/B03) doesn't count: it carries no
/// teardown logic of its own, so it isn't RAII evidence.
fn value_semantics_option(
    declaration: &TypeDeclaration,
    facts: &ProjectFacts<'_>,
) -> Option<MappingOption> {
    let methods = methods_of(declaration, facts);
    let has_destructor = methods
        .iter()
        .any(|method| method.kind == FunctionDeclarationKind::Destructor && !method.is_defaulted);
    if !has_destructor {
        return None;
    }

    let copy_constructor_needle = format!("(const {} &", declaration.name);
    let has_copy_constructor = methods.iter().any(|method| {
        method.kind == FunctionDeclarationKind::Constructor
            && method.signature.contains(&copy_constructor_needle)
    });
    let has_copy_assignment = methods
        .iter()
        .any(|method| method.name == "operator=" && method.kind == FunctionDeclarationKind::Method);

    if has_copy_constructor && has_copy_assignment {
        return Some(MappingOption {
            id: "clonagem-explicita-valor".to_owned(),
            label: "Código ponte: clonagem explícita".to_owned(),
            description: format!(
                "`{}` segue a Regra dos Três (construtor de cópia, `operator=` e destrutor \
                 fazem cópia profunda de um recurso próprio). Dart não tem construtor de cópia \
                 nem destrutor determinístico — atribuição é sempre referência. Código ponte \
                 necessário: um método `clonar()` explícito, chamado em todo call site do C++ \
                 original que copiava implicitamente.",
                declaration.name
            ),
            consequences: Vec::new(),
        });
    }

    Some(MappingOption {
        id: "dispose-explicito-raii".to_owned(),
        label: "Código ponte: dispose() explícito".to_owned(),
        description: format!(
            "`{}` declara um destrutor próprio (RAII): libera um recurso de forma \
             determinística, no ponto exato em que o objeto sai de escopo. Dart não tem \
             destrutor determinístico (`Finalizer` roda depois do GC, tarde demais para um \
             recurso do sistema operacional). Código ponte necessário: `dispose()`/`close()` \
             explícito, com todo escopo que dependia do RAII reescrito para chamá-lo.",
            declaration.name
        ),
        consequences: Vec::new(),
    })
}

/// A base-less class whose only members are virtual methods is a candidate
/// for `abstract interface class` — *unless* one of those virtual methods
/// isn't pure (has its own body), which Dart interfaces can't carry (case
/// B03). `None` when the class has no virtual methods at all (nothing to
/// decide here; falls through to the default class option).
fn interface_candidate_option(
    declaration: &TypeDeclaration,
    facts: &ProjectFacts<'_>,
) -> Option<MappingOption> {
    let methods = methods_of(declaration, facts);
    let virtual_methods: Vec<&FunctionDeclaration> = methods
        .iter()
        .filter(|method| method.kind == FunctionDeclarationKind::Method && method.is_virtual)
        .copied()
        .collect();
    if virtual_methods.is_empty() {
        return None;
    }

    let non_pure: Vec<&&FunctionDeclaration> = virtual_methods
        .iter()
        .filter(|method| !method.is_pure_virtual)
        .collect();

    if non_pure.is_empty() {
        return Some(MappingOption {
            id: "interface-pura".to_owned(),
            label: "Interface pura (`abstract interface class`)".to_owned(),
            description: format!(
                "Todo método virtual de `{}` é puro (sem corpo próprio): mapeia direto para \
                 `abstract interface class` em Dart, sem implementação herdável para carregar.",
                declaration.name
            ),
            consequences: Vec::new(),
        });
    }

    let consequences = non_pure
        .iter()
        .map(|method| Consequence {
            affected_type_usr: method.usr.clone(),
            description: format!(
                "`{}` tem corpo próprio (não é puro) — uma `abstract interface class` Dart não \
                 carrega implementação herdável, então esse corpo obriga `{}` a virar mixin, ou \
                 obriga o corpo a ser duplicado em cada implementador",
                method.name, declaration.name
            ),
        })
        .collect();

    Some(MappingOption {
        id: "mixin-com-implementacao-padrao".to_owned(),
        label: "Mixin com implementação padrão".to_owned(),
        description: format!(
            "`{}` parece uma interface, mas pelo menos um método virtual tem corpo próprio \
             (não é puro) — precisa virar mixin, não `abstract interface class`, para que a \
             implementação padrão sobreviva.",
            declaration.name
        ),
        consequences,
    })
}

/// The plain "one direct option" case (criterion 1 of US-7) — enriched, when
/// `facts` shows some other function taking `declaration` by non-const
/// reference and writing through it, with a `Consequence` documenting *why*
/// the class stays mutable (case B01 / Q9's "a choice for A restricts B",
/// mirrored: here it's a fact about B that restricts A's option, discovered
/// by looking outside `declaration`'s own file). The option itself doesn't
/// change shape — criterion 1 stays "no alternatives" — only its evidence
/// does, proving the cross-file check actually ran.
fn default_class_option(declaration: &TypeDeclaration, facts: &ProjectFacts<'_>) -> MappingOption {
    let mut consequences = Vec::new();
    if let Some(mutator) = external_mutating_reference(declaration, facts) {
        consequences.push(Consequence {
            affected_type_usr: declaration.usr.clone(),
            description: format!(
                "permanece uma classe Dart com campos mutáveis porque `{}` (`{}`) escreve em \
                 `{}` através de uma referência não-const",
                mutator.signature, mutator.file, declaration.name
            ),
        });
    }

    MappingOption {
        id: "classe-direta".to_owned(),
        label: format!("Classe Dart `{}`", declaration.name),
        description: format!(
            "Mapeamento direto: `{}` vira uma classe Dart com os mesmos campos.",
            declaration.name
        ),
        consequences,
    }
}

fn base_usrs_of(declaration: &TypeDeclaration, facts: &ProjectFacts<'_>) -> Vec<String> {
    facts
        .usages
        .iter()
        .filter(|usage| {
            usage.kind == TypeUsageKind::Inheritance
                && usage.file == declaration.file
                && usage.line >= declaration.line
                && usage.line <= declaration.end_line
        })
        .map(|usage| usage.type_usr.clone())
        .collect()
}

fn methods_of<'a>(
    declaration: &TypeDeclaration,
    facts: &ProjectFacts<'a>,
) -> Vec<&'a FunctionDeclaration> {
    facts
        .functions
        .iter()
        .filter(|function| function.owning_class_usr.as_deref() == Some(declaration.usr.as_str()))
        .collect()
}

/// Whether some function *not owned by `declaration` itself* takes it by
/// non-const reference — a real, if signature-only (not body-level),
/// forcing signal that the type has to stay mutable. `contains_non_const_reference`
/// is what tells `"Ponto3D & alvo"` apart from `"const Ponto3D & alvo"`
/// (a naive substring check on `"{name} &"` would match both, since the
/// `const` one contains it too).
fn external_mutating_reference<'a>(
    declaration: &TypeDeclaration,
    facts: &ProjectFacts<'a>,
) -> Option<&'a FunctionDeclaration> {
    facts.functions.iter().find(|function| {
        function.owning_class_usr.as_deref() != Some(declaration.usr.as_str())
            && contains_non_const_reference(&function.signature, &declaration.name)
    })
}

fn contains_non_const_reference(signature: &str, type_name: &str) -> bool {
    let needle = format!("{type_name} &");
    let mut search_from = 0;
    while let Some(relative_index) = signature
        .get(search_from..)
        .and_then(|rest| rest.find(&needle))
    {
        let index = search_from + relative_index;
        let preceded_by_const = signature[..index].trim_end().ends_with("const");
        if !preceded_by_const {
            return true;
        }
        search_from = index + needle.len();
    }
    false
}

/// Heuristic, file-level: any `#ifdef`/`#ifndef`/`#if `/`#elif` line
/// anywhere in `file` — a single `libclang` parse only ever sees the branch
/// that was actually compiled, so this is the only signal available that
/// *some other* branch might exist (case C03). Every other fixture in
/// `mapping-solver-fixtures/` uses `#pragma once` rather than an
/// `#ifndef`-style include guard, so this doesn't false-positive on guards
/// across the current corpus — a project that uses include-guard headers
/// would need a sharper check (e.g. ignoring the first guard pair), left for
/// when a real fixture needs it (AGENTS.md: no premature generality).
fn has_conditional_compilation(file: &str) -> bool {
    let Ok(contents) = fs::read_to_string(file) else {
        return false;
    };
    contents.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("#ifdef")
            || trimmed.starts_with("#ifndef")
            || trimmed.starts_with("#if ")
            || trimmed.starts_with("#elif")
    })
}

// ---------------------------------------------------------------------
// Function-level decisions: overloads, operators, templates, signatures
// ---------------------------------------------------------------------

/// The viable options for every `FunctionDeclaration` in `facts` sharing
/// `owning_class_usr`/`name` — an overload set (US-7 checklist items 6 and
/// 7: sobrecarga de função, sobrecarga de operadores). Never empty: even a
/// single, non-overloaded declaration gets one option back.
pub fn overload_options_for(
    owning_class_usr: Option<&str>,
    name: &str,
    facts: &ProjectFacts<'_>,
) -> Vec<MappingOption> {
    let group: Vec<&FunctionDeclaration> = facts
        .functions
        .iter()
        .filter(|function| {
            function.owning_class_usr.as_deref() == owning_class_usr && function.name == name
        })
        .collect();

    // Checked before the singleton fallback below: an operator is almost
    // always its own singleton "overload group" (`operator+` and
    // `operator==` are different *names*, so they never group together at
    // all) — `operator_option` already knows what to do with a
    // single-overload operator (case A07: maps directly) as well as a
    // genuinely overloaded or unsupported one, so it must run first or a
    // singleton operator would fall into the generic "no overload" case
    // below instead.
    if let Some(symbol) = name.strip_prefix("operator")
        && !symbol.is_empty()
    {
        let usrs: Vec<String> = group.iter().map(|f| f.usr.clone()).collect();
        return vec![operator_option(name, symbol, group.len(), &usrs, facts)];
    }

    if group.len() <= 1 {
        return vec![MappingOption {
            id: "assinatura-unica".to_owned(),
            label: format!("`{name}` sem sobrecarga"),
            description: format!("`{name}` não tem sobrecarga: mapeia direto, sem renomear."),
            consequences: Vec::new(),
        }];
    }

    // E13: C++ resolves `Fraction::Reduce()` (instance) and
    // `Fraction::Reduce(int&, int&)` (static) by call-site syntax alone —
    // legal, no real overload at all. Dart has no such second axis: a
    // `static` and a non-`static` member can never share a name in the same
    // class (`conflicting_static_and_instance`). Checked *before* the arity
    // check below, which would otherwise (wrongly) read these two as an
    // ordinary "differs only by parameter count" overload and choose
    // `"parametro-opcional"` — nonsensical here, since there's no single
    // Dart member the static and instance call sites could ever share.
    if group.iter().any(|f| f.is_static) && group.iter().any(|f| !f.is_static) {
        let usrs: Vec<String> = group.iter().map(|f| f.usr.clone()).collect();
        let mut consequences = call_site_consequences(&usrs, facts);
        for function in &group {
            consequences.push(Consequence {
                affected_type_usr: function.usr.clone(),
                description: format!(
                    "precisa de um nome próprio, distinto de `{name}`: `{}` ({})",
                    function.signature,
                    if function.is_static {
                        "estático"
                    } else {
                        "instância"
                    }
                ),
            });
        }
        return vec![MappingOption {
            id: "renomear-estatico-instancia".to_owned(),
            label: "Renomear (estático vs. instância)".to_owned(),
            description: format!(
                "`{name}` tem uma versão estática e uma de instância — Dart proíbe um membro \
                 estático e um de instância com o mesmo nome na mesma classe, então a versão \
                 estática precisa de um nome distinto."
            ),
            consequences,
        }];
    }

    let params_without_const = |signature: &str| signature.trim_end_matches(" const").to_owned();
    let arities: HashSet<usize> = group
        .iter()
        .map(|f| count_parameters(&f.signature))
        .collect();

    let usrs: Vec<String> = group.iter().map(|f| f.usr.clone()).collect();

    if group.len() == 2 {
        let (a, b) = (group[0], group[1]);
        let same_params = params_without_const(&a.signature) == params_without_const(&b.signature)
            || a.signature.trim_end() == format!("{} const", b.signature.trim_end())
            || b.signature.trim_end() == format!("{} const", a.signature.trim_end());
        let one_const_one_not =
            a.signature.trim_end().ends_with("const") != b.signature.trim_end().ends_with("const");
        if same_params && one_const_one_not {
            let mut consequences = vec![
                Consequence {
                    affected_type_usr: a.usr.clone(),
                    description: format!(
                        "precisa de um nome próprio, distinto de `{name}`: `{}`",
                        a.signature
                    ),
                },
                Consequence {
                    affected_type_usr: b.usr.clone(),
                    description: format!(
                        "precisa de um nome próprio, distinto de `{name}`: `{}`",
                        b.signature
                    ),
                },
            ];
            consequences.extend(call_site_consequences(&usrs, facts));
            return vec![MappingOption {
                id: "renomear-const-nao-const".to_owned(),
                label: "Renomear (const vs. não-const)".to_owned(),
                description: format!(
                    "`{name}` tem uma versão `const` e uma não-`const` com o mesmo efeito \
                     aparente, mas semânticas diferentes — Dart não despacha por const-ness do \
                     receptor, então as duas precisam de nomes distintos."
                ),
                consequences,
            }];
        }
    }

    if arities.len() > 1 {
        let mut consequences = call_site_consequences(&usrs, facts);
        for function in &group {
            consequences.push(Consequence {
                affected_type_usr: function.usr.clone(),
                description: format!(
                    "mapeia para `{name}` com parâmetro(s) opcional(is): `{}`",
                    function.signature
                ),
            });
        }
        return vec![MappingOption {
            id: "parametro-opcional".to_owned(),
            label: "Parâmetro opcional".to_owned(),
            description: format!(
                "As sobrecargas de `{name}` diferem em número de parâmetros: mapeiam direto \
                 para um único `{name}` com parâmetro(s) opcional(is) em Dart, sem precisar \
                 renomear."
            ),
            consequences,
        }];
    }

    let mut consequences = call_site_consequences(&usrs, facts);
    for function in &group {
        consequences.push(Consequence {
            affected_type_usr: function.usr.clone(),
            description: format!(
                "precisa de um nome próprio, distinto de `{name}`: `{}`",
                function.signature
            ),
        });
    }
    vec![MappingOption {
        id: "renomear-por-tipo".to_owned(),
        label: "Renomear por tipo".to_owned(),
        description: format!(
            "As sobrecargas de `{name}` têm a mesma aridade mas tipos de parâmetro diferentes — \
             Dart não despacha por tipo estático, então cada uma precisa de um nome distinto."
        ),
        consequences,
    }]
}

const DART_OPERATOR_SYMBOLS: &[&str] = &[
    "+", "-", "*", "/", "==", "[]", "[]=", "unary-", "<", "<=", ">", ">=",
];

fn operator_option(
    name: &str,
    symbol: &str,
    overload_count: usize,
    usrs: &[String],
    facts: &ProjectFacts<'_>,
) -> MappingOption {
    if overload_count == 1 && DART_OPERATOR_SYMBOLS.contains(&symbol) {
        return MappingOption {
            id: "operador-direto".to_owned(),
            label: format!("Operador Dart `{symbol}`"),
            description: format!(
                "`{name}` é um operador binário sem estado externo envolvido, e `{symbol}` está \
                 no subconjunto de operadores que Dart também sobrecarrega: mapeamento direto."
            ),
            consequences: Vec::new(),
        };
    }

    let mut consequences = call_site_consequences(usrs, facts);
    consequences.push(Consequence {
        affected_type_usr: usrs.first().cloned().unwrap_or_default(),
        description: format!(
            "`{symbol}` não está no subconjunto de operadores sobrecarregáveis de Dart (ou tem \
             mais de uma sobrecarga) — precisa de um método nomeado, não de um operador"
        ),
    });
    MappingOption {
        id: "operador-sem-equivalente-direto".to_owned(),
        label: "Código ponte: método nomeado".to_owned(),
        description: format!(
            "`{name}` não mapeia direto para um operador Dart — código ponte necessário: um \
             método nomeado no lugar do operador."
        ),
        consequences,
    }
}

fn count_parameters(signature: &str) -> usize {
    let Some(open) = signature.find('(') else {
        return 0;
    };
    let Some(close) = signature.rfind(')') else {
        return 0;
    };
    if close <= open {
        return 0;
    }
    let inner = signature[open + 1..close].trim();
    if inner.is_empty() {
        0
    } else {
        inner.split(',').count()
    }
}

/// Every caller of any USR in `usrs`, as a `Consequence` naming the file
/// that needs its call site rewritten — the cross-TU propagation case B04
/// exists to prove: a rename decided at the declaration doesn't stay local
/// to it. Deduplicated by caller `usr` (a caller with two call sites to the
/// same renamed group would otherwise get two identical consequences).
fn call_site_consequences(usrs: &[String], facts: &ProjectFacts<'_>) -> Vec<Consequence> {
    let mut seen = HashSet::new();
    let mut consequences = Vec::new();
    for edge in facts.calls {
        let CallResolution::Resolved { callee_usr, .. } = &edge.resolution else {
            continue;
        };
        if !usrs.iter().any(|usr| usr == callee_usr) {
            continue;
        }
        let Some(caller) = facts.functions.iter().find(|f| f.usr == edge.caller_usr) else {
            continue;
        };
        if !seen.insert(caller.usr.clone()) {
            continue;
        }
        consequences.push(Consequence {
            affected_type_usr: caller.usr.clone(),
            description: format!(
                "call site em `{}` (`{}`) precisa ser reescrito para o nome renomeado",
                caller.name, caller.file
            ),
        });
    }
    consequences
}

/// The monomorphization-vs-generics decision for a `FunctionTemplate` (US-7
/// checklist item 5). Distinguishes a template whose instantiation call
/// sites all live in one file (case A06: the decision is local, nothing
/// else in the project to check) from one whose call sites span more than
/// one file (case B06: a real decision needs every instantiation site
/// project-wide, and this function can only report the ones visible in
/// `facts`).
pub fn template_options_for(
    function: &FunctionDeclaration,
    facts: &ProjectFacts<'_>,
) -> Vec<MappingOption> {
    let caller_files: HashSet<&str> = facts
        .calls
        .iter()
        .filter_map(|edge| match &edge.resolution {
            CallResolution::Resolved { callee_usr, .. } if callee_usr == &function.usr => facts
                .functions
                .iter()
                .find(|f| f.usr == edge.caller_usr)
                .map(|f| f.file.as_str()),
            _ => None,
        })
        .collect();

    if caller_files.len() <= 1 {
        return vec![MappingOption {
            id: "monomorfizacao-local".to_owned(),
            label: "Monomorfização local".to_owned(),
            description: format!(
                "`{}` só tem instanciações visíveis em um arquivo ({} local(is)): dá para \
                 decidir localmente entre genéricos de Dart e monomorfização, sem varrer o \
                 resto do projeto.",
                function.name,
                caller_files.len()
            ),
            consequences: Vec::new(),
        }];
    }

    vec![MappingOption {
        id: "genericos-ou-monomorfizacao-decisao-global".to_owned(),
        label: "Decisão global: genéricos ou monomorfização".to_owned(),
        description: format!(
            "`{}` tem instanciações em {} arquivos diferentes nesta extração — uma decisão real \
             entre genéricos de Dart e monomorfização por instância precisa varrer TODO o \
             projeto por sites de instanciação, não só os visíveis aqui.",
            function.name,
            caller_files.len()
        ),
        consequences: Vec::new(),
    }]
}

// ---------------------------------------------------------------------
// Signature/body-level bridge detection (US-7 checklist items without a
// project type of their own to hang a rule off: pointers, fixed-width
// integers, floating point, non-local control flow, shared-memory
// concurrency)
// ---------------------------------------------------------------------

const FIXED_WIDTH_INT_NAMES: &[&str] = &[
    "int8_t",
    "uint8_t",
    "int16_t",
    "uint16_t",
    "int32_t",
    "uint32_t",
    "int64_t",
    "uint64_t",
    "unsigned char",
    "unsigned int",
    "unsigned long",
    "unsigned short",
];

/// Bridge-code detection that doesn't hang off a project `TypeDeclaration`
/// at all — a raw pointer parameter, a fixed-width integer, a lone `float`,
/// or (scanning the function's own source span, since none of this shows up
/// in `signature`) `setjmp`/`longjmp`, `goto`, or shared-memory
/// concurrency (`std::thread`/`std::mutex`). Always returns exactly one
/// option: either "nothing hard detected" or one bridge option whose
/// description lists every pattern that fired — these aren't alternatives
/// to pick between, they're cumulative requirements on the same function.
pub fn signature_options_for(function: &FunctionDeclaration) -> Vec<MappingOption> {
    let mut reasons = Vec::new();

    if function.signature.contains('*') && !function.signature.contains("operator*") {
        reasons.push(
            "ponteiro/aritmética de ponteiro na assinatura: sem equivalente em Dart, precisa de \
             `dart:ffi` (`Pointer<T>`)"
                .to_owned(),
        );
    }

    // `contains_word`/whole-phrase `contains` rather than a bare substring
    // search: `"int8_t"` is itself a substring of `"uint8_t"`, so a naive
    // `signature.contains(name)` over this list (checked in this order)
    // would misreport `uint8_t` as `int8_t`.
    if let Some(fixed_width) = FIXED_WIDTH_INT_NAMES.iter().find(|name| {
        if name.contains(' ') {
            function.signature.contains(**name)
        } else {
            contains_word(&function.signature, name)
        }
    }) {
        reasons.push(format!(
            "usa `{fixed_width}`: precisa de mascaramento explícito na emissão (o `int` de Dart \
             não estoura no mesmo ponto), não só troca de tipo"
        ));
    }

    if contains_word(&function.signature, "float") {
        reasons.push(
            "usa `float` (32 bits): Dart só tem `double` (64 bits) — mapear direto muda o \
             arredondamento observável"
                .to_owned(),
        );
    }

    let body = read_span(&function.file, function.line, function.end_line);
    if body.contains("setjmp") || body.contains("longjmp") {
        reasons.push(
            "usa `setjmp`/`longjmp`: desvio de controle não local, sem equivalente em Dart — \
             reescrever como máquina de estados ou recusar a conversão explicitamente"
                .to_owned(),
        );
    }
    if contains_word(&body, "goto") {
        reasons.push(
            "usa `goto`: sem equivalente em Dart para saída antecipada de função — reescrever \
             como `try`/`finally`"
                .to_owned(),
        );
    }
    if body.contains("std::thread") || body.contains("std::mutex") {
        reasons.push(
            "usa `std::thread`/`std::mutex`: isolates de Dart não compartilham memória — \
             reescrever em torno de troca de mensagens entre isolates"
                .to_owned(),
        );
    }

    if reasons.is_empty() {
        return vec![MappingOption {
            id: "assinatura-direta".to_owned(),
            label: "Assinatura direta".to_owned(),
            description: format!(
                "`{}` mapeia direto: nenhuma construção sem equivalente detectada.",
                function.name
            ),
            consequences: Vec::new(),
        }];
    }

    vec![MappingOption {
        id: "codigo-ponte".to_owned(),
        label: "Código ponte".to_owned(),
        description: format!(
            "`{}` precisa de código ponte: {}.",
            function.name,
            reasons.join("; ")
        ),
        consequences: Vec::new(),
    }]
}

/// What `lower::cpp` already knows about a pointer's pointee, before
/// `pointer_options_for` decides how (or whether) that pointer maps to
/// Dart. `Known` carries the pointee's own identity (`usr`/`name`) because
/// the solver's real answer — see that function's doc comment — is the
/// *enumerated* finite set of concrete types the pointer could hold, not
/// just a yes/no on whether one is representable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointeeShape {
    /// The pointee is a type this project's own analysis can already
    /// represent in full — this project's own `struct`/`class` (`usr`/
    /// `name` are its own), or `std::string`/`std::vector` (E05's library
    /// adapters, which have no project `usr` of their own to enumerate
    /// subtypes from — treated the same as a project class with no
    /// subclasses: the singleton set containing only itself).
    Known { usr: String, name: String },
    /// `void`, a scalar (`int`/`char`/`double`/...), or anything else this
    /// project's analysis can't already represent on its own.
    Opaque,
}

/// One concrete type a pointer could hold at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PossiblePointee {
    pub usr: String,
    pub name: String,
}

/// Decides how a raw C++ pointer (`T*`) maps to Dart, given what's already
/// known about its pointee `T` — item 2 of US-7's checklist
/// ("ponteiros/aritmética de ponteiros"), case A10
/// (`docs/mapping-solver-cases.md`), alongside `signature_options_for`'s
/// existing whole-signature textual detection (kept as-is: that one flags
/// *any* `*` in a signature as needing a bridge, a coarser, function-level
/// heuristic).
///
/// The set of concrete types a `T*` pointer's value could ever hold at
/// runtime is always finite: the C++ source that could construct one is
/// itself finite, and C++'s own static type system already guarantees the
/// pointer, whatever it holds, is either null or an object whose dynamic
/// type is `T` or a subtype of `T` declared somewhere in that same finite
/// source — there is no way for well-typed C++ to make it anything else.
/// The solver's actual answer, when `T` is itself representable
/// (`PointeeShape::Known`), *is* that set — `possible_pointee_types` walks
/// `facts`' own inheritance edges (the same ones `base_usrs_of` already
/// reads for E09's multiple-inheritance decision, just followed in the
/// opposite direction: from a base down to every class that extends it,
/// transitively) and returns every concrete type found, `T` itself
/// included. `facts` is optional because the two consumers of this
/// function sit at different points in the pipeline: `lower::cpp::lower_type`
/// (generation) only ever has the pointee's own `ir::Type` in hand, no
/// project-wide catalog, so it gets the correct-but-unenriched singleton
/// `[T]`; a caller with `ProjectFacts` (US-7's own decision/description
/// layer) gets the real, fully enumerated set. Either way the list is what
/// makes a plain nullable Dart reference (`T?`) sound to emit: Dart's own
/// single-reference polymorphism already covers every member of that set
/// through the one declared type `T`, so nothing about *which* member a
/// given pointer holds is ever a question `emit::dart` needs to answer.
///
/// When `T` itself isn't representable (`PointeeShape::Opaque` — `void`, a
/// scalar, or a type this project's own analysis already gave up on), that
/// finiteness guarantee stops helping: nothing in C++'s type system rules
/// out treating the pointer as a buffer (`ptr[i]`, `ptr + n`) instead of a
/// single value, and Dart has no address-arithmetic type to receive either
/// reading without `dart:ffi` — the same bridge answer
/// `signature_options_for` already gives case C01, unchanged here.
/// `owning_function` is the first, deliberately narrow step past pure CHA
/// toward the type-flow analysis `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`
/// (Parte 2) describes: when given, and the pointer's declared type resolves
/// to more than one possible concrete type, `pointer_options_for` also reads
/// `owning_function`'s own source body (`ProjectFacts` already carries
/// nothing about assignment sites — this reads straight from disk, the same
/// textual-evidence approach `string_usage_conflict` and
/// `signature_options_for`'s C01/C02/C03/C04 rules already use elsewhere in
/// this module) and narrows to whichever candidates the body actually
/// constructs (`new <Candidato>(...)`, case B07). No evidence found, or
/// `owning_function` omitted, leaves the full CHA enumeration untouched —
/// this narrows, it never invents a *wider* answer than CHA, and it never
/// narrows without positive evidence (soundness: see
/// `possible_pointee_types`'s own doc comment). When `owning_function`'s own
/// body has no construction evidence, this also tries one interprocedural
/// step (case B08, `docs/mapping-solver-cases.md`): if the body is a plain
/// forward of another function's return value (`return Callee(...)`, found
/// via `facts.calls`), the same narrowing recurses on `Callee`. Still only
/// return-forwarding — a value arriving through a parameter, a variable
/// assigned from outside the function, or a container, has no traceable
/// origin here and correctly falls back to full CHA, not because of an
/// artificial limit but for lack of evidence.
pub fn pointer_options_for(
    pointee: PointeeShape,
    facts: Option<&ProjectFacts<'_>>,
    owning_function: Option<&FunctionDeclaration>,
) -> Vec<MappingOption> {
    let PointeeShape::Known { usr, name } = pointee else {
        return vec![MappingOption {
            id: "ponte-dart-ffi".to_owned(),
            label: "Código ponte: dart:ffi".to_owned(),
            description: "o ponteiro aponta para um tipo que este projeto não representa (void, \
                          um escalar, ou algo que a própria análise já recusou) — nada garante \
                          que não seja usado como buffer/aritmética de endereço, e Dart não tem \
                          um tipo de referência que cubra esse caso sem `dart:ffi` (`Pointer<T>`)."
                .to_owned(),
            consequences: Vec::new(),
        }];
    };

    let mut possible_types = match facts {
        Some(facts) => possible_pointee_types(&usr, &name, facts),
        None => vec![PossiblePointee { usr, name }],
    };
    if let Some(owning_function) = owning_function {
        possible_types = narrow_by_construction_evidence(possible_types, owning_function, facts);
    }
    let names = possible_types
        .iter()
        .map(|pointee| pointee.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let consequences = possible_types
        .iter()
        .map(|pointee| Consequence {
            affected_type_usr: pointee.usr.clone(),
            description: format!(
                "`{}` é um dos tipos concretos que este ponteiro pode assumir em tempo de \
                 execução",
                pointee.name
            ),
        })
        .collect();

    vec![MappingOption {
        id: "referencia-anulavel".to_owned(),
        label: "Referência anulável".to_owned(),
        description: format!(
            "conjunto finito e conhecido de tipos possíveis: {names} — C++ garante \
             estaticamente que o ponteiro só pode ser nulo ou um destes, o mesmo conjunto que \
             a própria polimorfia de referência única do Dart já cobre com um único tipo \
             declarado: mapeia direto para uma referência anulável (`T?`)."
        ),
        consequences,
    }]
}

/// `usr`/`name`'s own declared type, plus every class in the project that
/// (transitively) inherits from it — always at least `[usr/name]` itself,
/// since a class with zero subclasses is still a finite set of exactly one.
/// Walks `facts.declarations` looking for any declaration whose own
/// `base_usrs_of` includes an already-found member of the set, growing the
/// frontier until nothing new turns up — a class hierarchy is a DAG in
/// valid C++ (no inheritance cycles), so this always terminates, bounded by
/// `facts.declarations.len()`.
fn possible_pointee_types(usr: &str, name: &str, facts: &ProjectFacts<'_>) -> Vec<PossiblePointee> {
    let mut found = vec![PossiblePointee {
        usr: usr.to_owned(),
        name: name.to_owned(),
    }];
    let mut seen_usrs: HashSet<&str> = [usr].into_iter().collect();
    let mut frontier = vec![usr.to_owned()];

    while let Some(base_usr) = frontier.pop() {
        for declaration in facts.declarations {
            if seen_usrs.contains(declaration.usr.as_str()) {
                continue;
            }
            if base_usrs_of(declaration, facts)
                .iter()
                .any(|base| base == &base_usr)
            {
                seen_usrs.insert(declaration.usr.as_str());
                frontier.push(declaration.usr.clone());
                found.push(PossiblePointee {
                    usr: declaration.usr.clone(),
                    name: declaration.name.clone(),
                });
            }
        }
    }

    found
}

/// Narrows `candidates` (CHA's full enumeration) to whichever ones there's
/// positive evidence of, trying two things in order:
///
/// 1. Construction evidence in `owning_function`'s own body (`new
///    Candidato(...)`) — case B07.
/// 2. Failing that, one interprocedural step: if the body is a plain
///    forward of another function's return value (`return Callee(...)`,
///    resolved via `facts.calls`), recurse the same narrowing on `Callee`
///    — case B08. `visited` guards against a forwarding cycle (`A` forwards
///    `B` forwards `A`) by refusing to revisit a function's `usr`.
///
/// Falls back to the full, unnarrowed `candidates` whenever neither finds
/// positive evidence: never returns a set this function isn't sure is at
/// least as wide as the truth.
fn narrow_by_construction_evidence(
    candidates: Vec<PossiblePointee>,
    owning_function: &FunctionDeclaration,
    facts: Option<&ProjectFacts<'_>>,
) -> Vec<PossiblePointee> {
    narrow_by_construction_evidence_visiting(
        candidates,
        owning_function,
        facts,
        &mut HashSet::new(),
    )
}

fn narrow_by_construction_evidence_visiting(
    candidates: Vec<PossiblePointee>,
    owning_function: &FunctionDeclaration,
    facts: Option<&ProjectFacts<'_>>,
    visited: &mut HashSet<String>,
) -> Vec<PossiblePointee> {
    if !visited.insert(owning_function.usr.clone()) {
        return candidates;
    }

    let body = read_span(
        &owning_function.file,
        owning_function.line,
        owning_function.end_line,
    );

    let constructed: Vec<PossiblePointee> = candidates
        .iter()
        .filter(|candidate| constructs_type(&body, &candidate.name))
        .cloned()
        .collect();
    if !constructed.is_empty() {
        return constructed;
    }

    if let Some(facts) = facts
        && let Some(callee) = forwarded_callee(&body, owning_function, facts)
    {
        let forwarded = narrow_by_construction_evidence_visiting(
            candidates.clone(),
            callee,
            Some(facts),
            visited,
        );
        if forwarded.len() < candidates.len() {
            return forwarded;
        }
    }

    candidates
}

/// The function `owning_function`'s body plainly forwards (`return
/// Callee(...)`), if any: an edge in `facts.calls` whose `caller_usr` is
/// `owning_function`'s own `usr`, resolved to a callee whose name the body
/// actually returns. Confirming both the call-graph edge *and* the textual
/// `return` pattern (rather than either alone) keeps this from mistaking an
/// unrelated call elsewhere in the body (logging, a side effect) for the
/// one that actually produces the returned pointer.
fn forwarded_callee<'a>(
    body: &str,
    owning_function: &FunctionDeclaration,
    facts: &ProjectFacts<'a>,
) -> Option<&'a FunctionDeclaration> {
    facts.calls.iter().find_map(|edge| {
        if edge.caller_usr != owning_function.usr {
            return None;
        }
        let CallResolution::Resolved { callee_usr, .. } = &edge.resolution else {
            return None;
        };
        let callee = facts.functions.iter().find(|f| &f.usr == callee_usr)?;
        if returns_via_call(body, &callee.name) {
            Some(callee)
        } else {
            None
        }
    })
}

/// Whether `body` contains a `return <callee_name>` — a `return` token
/// immediately followed by `callee_name` as its own token, mirroring
/// `constructs_type`'s `new`/type-name pairing.
fn returns_via_call(body: &str, callee_name: &str) -> bool {
    let tokens: Vec<&str> = body
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|token| !token.is_empty())
        .collect();
    tokens
        .windows(2)
        .any(|pair| pair[0] == "return" && pair[1] == callee_name)
}

/// Whether `body` contains a `new <type_name>` construction — a `new` token
/// immediately followed by `type_name` as its own token, so e.g. `new
/// Triangulo(...)` matches `Triangulo` but not `TrianguloEquilatero`.
fn constructs_type(body: &str, type_name: &str) -> bool {
    let tokens: Vec<&str> = body
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|token| !token.is_empty())
        .collect();
    tokens
        .windows(2)
        .any(|pair| pair[0] == "new" && pair[1] == type_name)
}

fn contains_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|token| token == word)
}

fn read_span(file: &str, start_line: u32, end_line: u32) -> String {
    let Ok(contents) = fs::read_to_string(file) else {
        return String::new();
    };
    contents
        .lines()
        .enumerate()
        .filter(|(index, _)| {
            let line_number = *index as u32 + 1;
            line_number >= start_line && line_number <= end_line
        })
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------
// Project-wide, cross-cutting checks — no single type or function is the
// right place to hang these on
// ---------------------------------------------------------------------

/// Case B05: `std::string` used as text (no byte-level access) in one
/// function and as an opaque binary buffer (`.data()`/`memcpy`/`fwrite`) in
/// another. `String` vs. `Uint8List` isn't decidable from either function
/// alone — this scans every function in `facts` that mentions `std::string`
/// in its own signature and looks for both kinds of evidence project-wide.
/// Coarser than real dataflow (it doesn't trace that the *same* value flows
/// between the two functions, only that the pattern coexists in the
/// project) — real tracing is future work, not yet needed by any case in
/// the corpus.
pub fn string_usage_conflict(facts: &ProjectFacts<'_>) -> Option<MappingOption> {
    let mut text_evidence: Option<&FunctionDeclaration> = None;
    let mut binary_evidence: Option<&FunctionDeclaration> = None;

    for function in facts.functions {
        if !function.signature.contains("std::string") {
            continue;
        }
        let body = read_span(&function.file, function.line, function.end_line);
        let looks_binary =
            body.contains(".data()") || body.contains("memcpy") || body.contains("fwrite");
        if looks_binary {
            binary_evidence.get_or_insert(function);
        } else {
            text_evidence.get_or_insert(function);
        }
    }

    let (text_fn, binary_fn) = (text_evidence?, binary_evidence?);
    Some(MappingOption {
        id: "decisao-de-produto-string-vs-bytes".to_owned(),
        label: "Decisão de produto: String vs. Uint8List".to_owned(),
        description: "`std::string` é usado como texto em pelo menos um lugar do projeto e como \
                       buffer binário opaco em outro — a decisão `String` vs. `Uint8List` não é \
                       por tipo declarado, é por uso, e os usos vivem em funções diferentes."
            .to_owned(),
        consequences: vec![
            Consequence {
                affected_type_usr: text_fn.usr.clone(),
                description: format!(
                    "usa `std::string` como texto: `{}` (`{}`)",
                    text_fn.signature, text_fn.file
                ),
            },
            Consequence {
                affected_type_usr: binary_fn.usr.clone(),
                description: format!(
                    "usa `std::string` como buffer binário opaco: `{}` (`{}`)",
                    binary_fn.signature, binary_fn.file
                ),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declaration(kind: TypeDeclarationKind, name: &str) -> TypeDeclaration {
        TypeDeclaration {
            name: name.to_owned(),
            kind,
            namespace: String::new(),
            file: "/project/input-source/src/ponto.hpp".to_owned(),
            line: 3,
            column: 8,
            end_line: 6,
            end_column: 2,
            usr: format!("c:@S@{name}"),
        }
    }

    /// Criterion 1 of US-7: a struct without multiple inheritance gets
    /// exactly one option, no alternatives.
    #[test]
    fn a_struct_gets_exactly_one_direct_mapping_option() {
        let ponto = declaration(TypeDeclarationKind::Struct, "Ponto");
        let facts = ProjectFacts::new(&[]);
        let options = options_for(&ponto, &facts, &[]);
        assert_eq!(
            options.len(),
            1,
            "expected exactly one option, got {options:?}"
        );
        assert_eq!(options[0].id, "classe-direta");
    }

    #[test]
    fn a_class_also_gets_exactly_one_direct_mapping_option() {
        let shape = declaration(TypeDeclarationKind::Class, "Shape");
        let facts = ProjectFacts::new(&[]);
        let options = options_for(&shape, &facts, &[]);
        assert_eq!(
            options.len(),
            1,
            "expected exactly one option, got {options:?}"
        );
        assert_eq!(options[0].id, "classe-direta");
    }

    /// Q9 / criterion 5 of US-7: the option list is never empty, even for a
    /// kind with no direct mapping yet.
    #[test]
    fn a_kind_with_no_direct_mapping_still_gets_a_non_empty_bridge_option() {
        let alias = declaration(TypeDeclarationKind::TypeAlias, "Area");
        let facts = ProjectFacts::new(&[]);
        let options = options_for(&alias, &facts, &[]);
        assert!(
            !options.is_empty(),
            "the option list must never be empty (Q9)"
        );
        assert_eq!(options[0].id, "codigo-ponte");
        assert!(
            options[0].description.contains("Area"),
            "the bridge option should name the type it's standing in for, got: {}",
            options[0].description
        );
    }

    #[test]
    fn a_union_always_gets_a_bridge_option() {
        let valor = declaration(TypeDeclarationKind::Union, "ValorNumerico");
        let facts = ProjectFacts::new(&[]);
        let options = options_for(&valor, &facts, &[]);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].id, "uniao-com-tag");
    }

    /// A10: a pointer to a type this project already represents has a
    /// statically finite set of possible values (null, or that type/a
    /// subtype) — resolved for real, a nullable reference, not a bridge.
    /// No `ProjectFacts` in hand here (the `lower::cpp::lower_type` case,
    /// which never has a project-wide catalog either) — the answer still
    /// has to be a genuine, if unenriched, list: just the pointee itself.
    #[test]
    fn a10_pointer_to_a_known_type_with_no_facts_is_a_nullable_reference_to_just_itself() {
        let options = pointer_options_for(
            PointeeShape::Known {
                usr: "c:@S@Nota".to_owned(),
                name: "Nota".to_owned(),
            },
            None,
            None,
        );
        assert_eq!(options.len(), 1, "{options:?}");
        assert_eq!(options[0].id, "referencia-anulavel");
        assert_eq!(options[0].consequences.len(), 1, "{options:?}");
        assert_eq!(options[0].consequences[0].affected_type_usr, "c:@S@Nota");
    }

    /// The real payload: with `ProjectFacts` in hand, the solver walks the
    /// project's own inheritance edges and enumerates every subclass —
    /// `Forma` has two, `Circulo` and `Quadrado`, neither related to the
    /// other except through `Forma`. A pointer to `Forma` can concretely be
    /// any of the three (or null); the solver's list must name all three,
    /// not just `Forma`.
    #[test]
    fn a10_pointer_to_a_polymorphic_base_enumerates_every_subclass() {
        // `declaration()` fixes file/line/end_line the same for every call
        // — fine for the single-declaration tests elsewhere in this module,
        // but `base_usrs_of` matches an inheritance usage to its derived
        // class by `(file, line range)`, so three declarations here need
        // three distinct spans or each would spuriously look like it
        // inherits from itself.
        let mut forma = declaration(TypeDeclarationKind::Class, "Forma");
        forma.file = "/project/input-source/src/forma.hpp".to_owned();
        let mut circulo = declaration(TypeDeclarationKind::Class, "Circulo");
        circulo.file = "/project/input-source/src/circulo.hpp".to_owned();
        let mut quadrado = declaration(TypeDeclarationKind::Class, "Quadrado");
        quadrado.file = "/project/input-source/src/quadrado.hpp".to_owned();
        let usages = vec![
            TypeUsage {
                type_usr: forma.usr.clone(),
                kind: TypeUsageKind::Inheritance,
                file: circulo.file.clone(),
                line: circulo.line,
                column: 1,
            },
            TypeUsage {
                type_usr: forma.usr.clone(),
                kind: TypeUsageKind::Inheritance,
                file: quadrado.file.clone(),
                line: quadrado.line,
                column: 1,
            },
        ];
        let declarations = vec![forma.clone(), circulo, quadrado];
        let facts = ProjectFacts {
            declarations: &declarations,
            usages: &usages,
            functions: &[],
            calls: &[],
        };

        let options = pointer_options_for(
            PointeeShape::Known {
                usr: forma.usr.clone(),
                name: forma.name.clone(),
            },
            Some(&facts),
            None,
        );
        assert_eq!(options.len(), 1, "{options:?}");
        assert_eq!(options[0].id, "referencia-anulavel");
        let mut names: Vec<&str> = options[0]
            .consequences
            .iter()
            .map(|c| {
                declarations
                    .iter()
                    .find(|d| d.usr == c.affected_type_usr)
                    .map(|d| d.name.as_str())
                    .unwrap_or("?")
            })
            .collect();
        names.sort_unstable();
        assert_eq!(names, vec!["Circulo", "Forma", "Quadrado"], "{options:?}");
    }

    /// C01's counterpart at the type level: a pointer to `void`/a scalar/
    /// anything else this project can't represent has no such guarantee —
    /// still needs `dart:ffi`.
    #[test]
    fn pointer_to_an_opaque_type_still_needs_a_dart_ffi_bridge() {
        let options = pointer_options_for(PointeeShape::Opaque, None, None);
        assert_eq!(options.len(), 1, "{options:?}");
        assert_eq!(options[0].id, "ponte-dart-ffi");
        assert!(
            options[0].description.contains("dart:ffi"),
            "{:?}",
            options[0]
        );
    }

    fn free_function(name: &str, usr: &str, file: &str) -> FunctionDeclaration {
        FunctionDeclaration {
            name: name.to_owned(),
            kind: FunctionDeclarationKind::FreeFunction,
            namespace: String::new(),
            owning_class_usr: None,
            signature: format!("{name}()"),
            file: file.to_owned(),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
            usr: usr.to_owned(),
            is_static: false,
            is_virtual: false,
            is_pure_virtual: false,
            is_defaulted: false,
            overridden_usrs: Vec::new(),
        }
    }

    fn resolved_call(caller_usr: &str, callee_usr: &str) -> CallEdge {
        CallEdge {
            caller_usr: caller_usr.to_owned(),
            resolution: CallResolution::Resolved {
                callee_usr: callee_usr.to_owned(),
                is_dynamic_dispatch: false,
            },
            file: "irrelevant".to_owned(),
            line: 1,
            column: 1,
        }
    }

    /// The half of `template_options_for` no fixture in
    /// `mapping-solver-fixtures/` exercises (A06 only has same-file
    /// instantiations): call sites spread across more than one file mean a
    /// real generics-vs-monomorphization decision needs a project-wide scan,
    /// not just what's visible here.
    #[test]
    fn a_template_instantiated_from_two_files_needs_a_global_decision() {
        let template = free_function("maior", "c:@FT@maior", "/project/src/maior.hpp");
        let caller_a = free_function("usarComInt", "c:@F@usarComInt", "/project/src/a.cpp");
        let caller_b = free_function("usarComDouble", "c:@F@usarComDouble", "/project/src/b.cpp");
        let functions = vec![template.clone(), caller_a.clone(), caller_b.clone()];
        let calls = vec![
            resolved_call(&caller_a.usr, &template.usr),
            resolved_call(&caller_b.usr, &template.usr),
        ];
        let facts = ProjectFacts {
            declarations: &[],
            usages: &[],
            functions: &functions,
            calls: &calls,
        };

        let options = template_options_for(&template, &facts);
        assert_eq!(options.len(), 1, "{options:?}");
        assert_eq!(options[0].id, "genericos-ou-monomorfizacao-decisao-global");
    }
}
