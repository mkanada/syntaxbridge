//! Minimal slice of US-7 (mapeamento de tipos C++ → Dart) that E03
//! (`docs/plans/primeiro-corte-e01-e03.md` §7 PR5) needs: a `struct`/`class`
//! gets exactly one viable option — a direct Dart class mapping — and any
//! other kind gets a bridge-code placeholder, never an empty list (Q9 in
//! `docs/plans/User Steps.md`: the option set is never empty, because when
//! no direct mapping exists the product offers bridge code instead of
//! declaring the type unconvertible).
//!
//! [`options_for`] already has the shape a real constraint solver will need
//! later (dependent on `catalog`/`decisions`, not just the one declaration) —
//! E09 (herança múltipla) is what actually exercises that shape; nothing
//! here does yet, and that's deliberate: trading a validator for a solver
//! later shouldn't change this function's signature or its callers.

use serde::{Deserialize, Serialize};

use crate::type_catalog::{TypeDeclaration, TypeDeclarationKind};

/// What choosing an option changes about another type — e.g. "escolher
/// mixin para `Base` obriga `Derived` a virar `with Base`". Structured (not
/// free text) so criterion 3 of US-7 ("uma opção que tornaria outro tipo não
/// convertível não é oferecida") is machine-checkable once a real solver
/// exists.
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

/// The viable mapping options for `declaration`, filtered by global
/// feasibility (Q9) once a real solver exists — `catalog`/`decisions` are
/// accepted now so that future change doesn't touch this function's
/// signature, even though neither is consulted yet (nothing in E01–E03 has
/// more than one viable option to filter between).
///
/// Never returns an empty `Vec` (criterion 5 of US-7 / Q9): a type with no
/// direct mapping still gets a bridge-code option, with a reason.
pub fn options_for(
    declaration: &TypeDeclaration,
    _catalog: &[TypeDeclaration],
    _decisions: &[MappingDecision],
) -> Vec<MappingOption> {
    match declaration.kind {
        // Criterion 1 of US-7: a struct/class without multiple inheritance
        // gets a direct mapping, no alternatives offered. "Sem herança
        // múltipla" isn't checked here yet — no E01–E03 fixture declares
        // any inheritance at all, so there is nothing to distinguish; E09
        // is where a real base-class count enters this decision.
        TypeDeclarationKind::Struct | TypeDeclarationKind::Class => vec![MappingOption {
            id: "classe-direta".to_owned(),
            label: format!("Classe Dart `{}`", declaration.name),
            description: format!(
                "Mapeamento direto: `{}` vira uma classe Dart com os mesmos campos.",
                declaration.name
            ),
            consequences: Vec::new(),
        }],
        _ => vec![MappingOption {
            id: "codigo-ponte".to_owned(),
            label: "Código ponte".to_owned(),
            description: format!(
                "Nenhum mapeamento direto existe ainda para `{}` ({:?}) — código ponte \
                 mantém a conversão possível até que um mapeamento dedicado seja \
                 implementado.",
                declaration.name, declaration.kind
            ),
            consequences: Vec::new(),
        }],
    }
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
        let options = options_for(&ponto, &[], &[]);
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
        let options = options_for(&shape, &[], &[]);
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
        let options = options_for(&alias, &[], &[]);
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
}
