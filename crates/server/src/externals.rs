//! "Extern" — code the user (or an auto-detected signal) decides not to
//! transpile, emitted as a mock (a plausible default value, execution
//! keeps going) instead of the `Unsupported` bailout every other
//! unrepresentable construct uses. Design and the decisions behind it:
//! `docs/plans/lista-de-externos.md`.

use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::function_catalog::{CallEdge, CallResolution, FunctionDeclaration};
use crate::type_catalog::TypeDeclaration;

/// A user's explicit decision for one usr — `external: true` forces it into
/// the effective external set even if nothing else would match it;
/// `external: false` forces it *out*, overriding a matching regex or an
/// auto-detected candidate (decision 5 of the design doc: manual always has
/// the last word). Keyed by `usr`, upserted, never wholesale-replaced on
/// re-ingestion — same persistence shape as `mapping::MappingDecision`,
/// for the same reason: this is a user decision, not derived catalog data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalMark {
    pub usr: String,
    pub external: bool,
    pub decided_at: String,
}

/// One name-regex rule (decision 6 of the design doc: matches the qualified
/// C++ name, `namespace::Class::method` / `namespace::function`) — matches
/// are always recomputed live against the current catalog, never
/// materialized. `id` is assigned by SQLite (`AUTOINCREMENT`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameRegexRule {
    pub id: i64,
    pub pattern: String,
    pub created_at: String,
}

/// One path-regex rule (decision 6 of the design doc: matches a
/// declaration's file path) — same live-recompute semantics as
/// [`NameRegexRule`], kept as a distinct type rather than a shared shape so
/// the two match targets can never be mixed up by accident.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathRegexRule {
    pub id: i64,
    pub pattern: String,
    pub created_at: String,
}

/// A whole file marked external — persistent, unlike the one-time
/// `expand_file_mark`/`expand_type_mark` snapshot a "mark this type
/// external" action still uses. Reverses decision 3 of the design doc
/// (`docs/plans/lista-de-externos.md`, "Cascata é foto, não vínculo") for
/// files specifically, per `docs/prompts/2026-08-19-mudanca-interacao.md`
/// item 3: every declaration currently in `file`, *and any declared there
/// later*, is external for as long as this mark exists. Presence in the
/// list is the mark — there is no `external: false` entry, unlike
/// [`ExternalMark`]; removing the file's entry is how you unmark it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileMark {
    pub file: String,
    pub decided_at: String,
}

/// Why a usr is (or would be) external — a usr can carry more than one at
/// once (e.g. matched by a name regex *and* manually excluded); `sources`
/// on [`ExternalStatus`] keeps every one of them, not just the one that
/// decided the final `effective` value, so the UI can show "would be
/// external because X, but you excluded it" instead of just a bare yes/no.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ExternalSource {
    /// A manual mark with `external: true`.
    ManualInclude,
    /// A manual mark with `external: false` — decision 5
    /// (`docs/plans/lista-de-externos.md`): always wins over every other
    /// source below, in both directions.
    ManualExclude,
    NameRegex {
        pattern: String,
    },
    PathRegex {
        pattern: String,
    },
    /// The declaration's file has a persistent [`FileMark`] (item 3,
    /// `docs/prompts/2026-08-19-mudanca-interacao.md`).
    FileMark {
        file: String,
    },
    /// A free function this project declares but never defines in any
    /// parsed compilation unit (`function_catalog`'s prototype cataloging,
    /// decision 2). Decision 3: active by default, exactly like every
    /// other source here — nothing special-cases it in `effective`.
    AutoUndefinedFunction,
}

/// One usr's full external status — see [`effective_external_set`]'s doc
/// comment for how `sources` combine into `effective`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalStatus {
    pub usr: String,
    pub effective: bool,
    pub sources: Vec<ExternalSource>,
}

/// The qualified name a name-regex rule matches against for a type:
/// `namespace::Name`, or just `Name` with no enclosing namespace.
fn qualified_type_name(declaration: &TypeDeclaration) -> String {
    if declaration.namespace.is_empty() {
        declaration.name.clone()
    } else {
        format!("{}::{}", declaration.namespace, declaration.name)
    }
}

/// The qualified name a name-regex rule matches against for a function:
/// `namespace::OwningClass::name` for a method (`owning_class_usr` resolved
/// against `types_by_usr`, mirroring `function_catalog::describe_function`'s
/// own `qualified_segments` construction at extraction time — not stored on
/// `FunctionDeclaration` itself, so it's rebuilt here from the same parts),
/// or `namespace::name` for a free function.
fn qualified_function_name(
    declaration: &FunctionDeclaration,
    types_by_usr: &HashMap<&str, &TypeDeclaration>,
) -> String {
    let mut segments: Vec<&str> = Vec::new();
    if !declaration.namespace.is_empty() {
        segments.push(&declaration.namespace);
    }
    let owning_class_name = declaration
        .owning_class_usr
        .as_deref()
        .and_then(|usr| types_by_usr.get(usr))
        .map(|type_declaration| type_declaration.name.as_str());
    if let Some(name) = owning_class_name {
        segments.push(name);
    }
    segments.push(&declaration.name);
    segments.join("::")
}

/// A usr this project calls (`CallResolution::Resolved`) but never defines
/// anywhere — decision 2 (`docs/plans/lista-de-externos.md`): the auto-
/// detection signal for function-level "externo", built entirely from
/// already-persisted catalogs (no new libclang pass). A usr with **no**
/// `FunctionDeclaration` at all (an unresolvable reference, shouldn't
/// normally happen for a `Resolved` call) counts the same as one whose
/// declaration exists but has `has_definition: false` — both mean "this
/// project has no body for it".
fn auto_undefined_function_usrs(
    function_declarations: &[FunctionDeclaration],
    call_edges: &[CallEdge],
) -> HashSet<String> {
    let defined_usrs: HashSet<&str> = function_declarations
        .iter()
        .filter(|declaration| declaration.has_definition)
        .map(|declaration| declaration.usr.as_str())
        .collect();

    call_edges
        .iter()
        .filter_map(|edge| match &edge.resolution {
            CallResolution::Resolved { callee_usr, .. } => Some(callee_usr.clone()),
            CallResolution::Unresolved { .. } => None,
        })
        .filter(|callee_usr| !defined_usrs.contains(callee_usr.as_str()))
        .collect()
}

/// The full formula (`docs/plans/lista-de-externos.md`):
///
/// ```text
/// effective(usr) =
///     ( name_regex_matches(usr) OR path_regex_matches(usr) OR file_mark_matches(usr)
///       OR auto_detected(usr) OR manual_mark(usr) == true )
///     AND NOT manual_mark(usr) == false
/// ```
///
/// Returns one [`ExternalStatus`] per usr that has *any* source at all
/// (manual, regex, file mark, or auto-detected) — a usr nothing here ever
/// mentions is simply not part of the result, not an entry with
/// `effective: false` and an empty `sources`. Regex patterns are assumed
/// already valid (`validate_regex_pattern` is what the write path — not
/// this pure function — calls to guarantee that before a pattern is ever
/// persisted); a pattern that still somehow fails to compile is skipped
/// rather than panicking or failing the whole computation, consistent with
/// this function never returning a `Result`.
pub fn effective_external_set(
    type_declarations: &[TypeDeclaration],
    function_declarations: &[FunctionDeclaration],
    call_edges: &[CallEdge],
    name_regexes: &[NameRegexRule],
    path_regexes: &[PathRegexRule],
    file_marks: &[FileMark],
    manual_marks: &[ExternalMark],
) -> Vec<ExternalStatus> {
    let types_by_usr: HashMap<&str, &TypeDeclaration> = type_declarations
        .iter()
        .map(|declaration| (declaration.usr.as_str(), declaration))
        .collect();

    let compiled_name_regexes: Vec<(Regex, &str)> = name_regexes
        .iter()
        .filter_map(|rule| {
            Regex::new(&rule.pattern)
                .ok()
                .map(|re| (re, rule.pattern.as_str()))
        })
        .collect();
    let compiled_path_regexes: Vec<(Regex, &str)> = path_regexes
        .iter()
        .filter_map(|rule| {
            Regex::new(&rule.pattern)
                .ok()
                .map(|re| (re, rule.pattern.as_str()))
        })
        .collect();

    let auto_undefined = auto_undefined_function_usrs(function_declarations, call_edges);
    let marked_files: HashSet<&str> = file_marks.iter().map(|mark| mark.file.as_str()).collect();

    let mut sources_by_usr: HashMap<String, Vec<ExternalSource>> = HashMap::new();

    for declaration in type_declarations {
        let qualified_name = qualified_type_name(declaration);
        for (regex, pattern) in &compiled_name_regexes {
            if regex.is_match(&qualified_name) {
                sources_by_usr
                    .entry(declaration.usr.clone())
                    .or_default()
                    .push(ExternalSource::NameRegex {
                        pattern: (*pattern).to_owned(),
                    });
            }
        }
        for (regex, pattern) in &compiled_path_regexes {
            if regex.is_match(&declaration.file) {
                sources_by_usr
                    .entry(declaration.usr.clone())
                    .or_default()
                    .push(ExternalSource::PathRegex {
                        pattern: (*pattern).to_owned(),
                    });
            }
        }
        if marked_files.contains(declaration.file.as_str()) {
            sources_by_usr
                .entry(declaration.usr.clone())
                .or_default()
                .push(ExternalSource::FileMark {
                    file: declaration.file.clone(),
                });
        }
    }

    for declaration in function_declarations {
        let qualified_name = qualified_function_name(declaration, &types_by_usr);
        for (regex, pattern) in &compiled_name_regexes {
            if regex.is_match(&qualified_name) {
                sources_by_usr
                    .entry(declaration.usr.clone())
                    .or_default()
                    .push(ExternalSource::NameRegex {
                        pattern: (*pattern).to_owned(),
                    });
            }
        }
        for (regex, pattern) in &compiled_path_regexes {
            if regex.is_match(&declaration.file) {
                sources_by_usr
                    .entry(declaration.usr.clone())
                    .or_default()
                    .push(ExternalSource::PathRegex {
                        pattern: (*pattern).to_owned(),
                    });
            }
        }
        if marked_files.contains(declaration.file.as_str()) {
            sources_by_usr
                .entry(declaration.usr.clone())
                .or_default()
                .push(ExternalSource::FileMark {
                    file: declaration.file.clone(),
                });
        }
        if auto_undefined.contains(&declaration.usr) {
            sources_by_usr
                .entry(declaration.usr.clone())
                .or_default()
                .push(ExternalSource::AutoUndefinedFunction);
        }
    }
    // A usr auto-detected purely from `call_edges` (no `FunctionDeclaration`
    // at all — the "unresolvable reference" edge case its own doc comment
    // names) still needs an entry: the loop above only ever visits usrs
    // that already have a declaration.
    for usr in &auto_undefined {
        sources_by_usr
            .entry(usr.clone())
            .or_default()
            .push(ExternalSource::AutoUndefinedFunction);
    }

    for mark in manual_marks {
        let source = if mark.external {
            ExternalSource::ManualInclude
        } else {
            ExternalSource::ManualExclude
        };
        sources_by_usr
            .entry(mark.usr.clone())
            .or_default()
            .push(source);
    }

    let manual_by_usr: HashMap<&str, bool> = manual_marks
        .iter()
        .map(|mark| (mark.usr.as_str(), mark.external))
        .collect();

    let mut statuses: Vec<ExternalStatus> = sources_by_usr
        .into_iter()
        .map(|(usr, mut sources)| {
            // Dedup: an auto-detected function usr is pushed to twice — once
            // by the `function_declarations` loop above, once by the
            // trailing `auto_undefined` loop that also covers a usr with no
            // declaration at all — simpler to sort+dedup once here than to
            // thread an "already pushed" guard through every push site.
            sources.sort();
            sources.dedup();

            let non_manual_match = sources.iter().any(|source| {
                !matches!(
                    source,
                    ExternalSource::ManualInclude | ExternalSource::ManualExclude
                )
            });
            let effective = match manual_by_usr.get(usr.as_str()) {
                Some(&forced) => forced,
                None => non_manual_match,
            };

            ExternalStatus {
                usr,
                effective,
                sources,
            }
        })
        .collect();
    statuses.sort_by(|a, b| a.usr.cmp(&b.usr));
    statuses
}

/// `type_usr` itself, plus every method `function_declarations` attributes
/// to it (`owning_class_usr == type_usr`) — used to expand a "mark this
/// type external" action into the individual usrs to write as manual marks.
/// Types keep decision 3's original one-time-snapshot semantics
/// (`docs/plans/lista-de-externos.md`); only files were reversed to a
/// persistent [`FileMark`] (item 3, `docs/prompts/2026-08-19-mudanca-interacao.md`).
pub fn expand_type_mark(
    type_usr: &str,
    function_declarations: &[FunctionDeclaration],
) -> Vec<String> {
    let mut usrs: Vec<String> = std::iter::once(type_usr.to_owned())
        .chain(
            function_declarations
                .iter()
                .filter(|declaration| declaration.owning_class_usr.as_deref() == Some(type_usr))
                .map(|declaration| declaration.usr.clone()),
        )
        .collect();
    usrs.sort();
    usrs.dedup();
    usrs
}

/// The write path (a future HTTP handler, M5) calls this before persisting
/// a new regex rule — refusing an invalid pattern at the door is what keeps
/// [`effective_external_set`]'s own "skip a pattern that fails to compile"
/// fallback purely defensive instead of a real, silent way to lose a rule.
pub fn validate_regex_pattern(pattern: &str) -> Result<(), String> {
    Regex::new(pattern)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::function_catalog::FunctionDeclarationKind;
    use crate::type_catalog::TypeDeclarationKind;

    fn free_function(usr: &str, name: &str, namespace: &str, file: &str) -> FunctionDeclaration {
        FunctionDeclaration {
            name: name.to_owned(),
            kind: FunctionDeclarationKind::FreeFunction,
            namespace: namespace.to_owned(),
            owning_class_usr: None,
            signature: format!("void {name}()"),
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
            has_definition: true,
        }
    }

    fn method(
        usr: &str,
        name: &str,
        owning_class_usr: &str,
        namespace: &str,
    ) -> FunctionDeclaration {
        FunctionDeclaration {
            owning_class_usr: Some(owning_class_usr.to_owned()),
            kind: FunctionDeclarationKind::Method,
            ..free_function(usr, name, namespace, "/project/input-source/src/shape.h")
        }
    }

    fn type_declaration(usr: &str, name: &str, namespace: &str, file: &str) -> TypeDeclaration {
        TypeDeclaration {
            name: name.to_owned(),
            kind: TypeDeclarationKind::Class,
            namespace: namespace.to_owned(),
            file: file.to_owned(),
            line: 1,
            column: 1,
            end_line: 5,
            end_column: 1,
            usr: usr.to_owned(),
        }
    }

    fn name_regex(pattern: &str) -> NameRegexRule {
        NameRegexRule {
            id: 1,
            pattern: pattern.to_owned(),
            created_at: "2026-08-17T00:00:00Z".to_owned(),
        }
    }

    fn path_regex(pattern: &str) -> PathRegexRule {
        PathRegexRule {
            id: 1,
            pattern: pattern.to_owned(),
            created_at: "2026-08-17T00:00:00Z".to_owned(),
        }
    }

    fn manual_mark(usr: &str, external: bool) -> ExternalMark {
        ExternalMark {
            usr: usr.to_owned(),
            external,
            decided_at: "2026-08-17T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn a_manual_include_with_no_other_source_is_effective() {
        let statuses = effective_external_set(
            &[],
            &[free_function("c:@F@f#", "f", "", "/project/f.cpp")],
            &[],
            &[],
            &[],
            &[],
            &[manual_mark("c:@F@f#", true)],
        );
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].effective);
        assert_eq!(statuses[0].sources, vec![ExternalSource::ManualInclude]);
    }

    #[test]
    fn a_usr_matched_by_nothing_is_absent_from_the_result() {
        let statuses = effective_external_set(
            &[],
            &[free_function("c:@F@f#", "f", "", "/project/f.cpp")],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(statuses.is_empty(), "{statuses:#?}");
    }

    #[test]
    fn a_name_regex_matches_a_namespaced_types_qualified_name() {
        let statuses = effective_external_set(
            &[type_declaration(
                "c:@N@humlib@S@Foo",
                "Foo",
                "humlib",
                "/project/input-source/third_party/humlib/humlib.h",
            )],
            &[],
            &[],
            &[name_regex("^humlib::")],
            &[],
            &[],
            &[],
        );
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].effective);
        assert_eq!(
            statuses[0].sources,
            vec![ExternalSource::NameRegex {
                pattern: "^humlib::".to_owned()
            }]
        );
    }

    #[test]
    fn a_name_regex_matches_a_methods_qualified_name_via_its_owning_class() {
        let shape = type_declaration(
            "c:@S@Shape",
            "Shape",
            "",
            "/project/input-source/src/shape.h",
        );
        let area = method("c:@S@Shape@F@area#", "area", "c:@S@Shape", "");
        let statuses = effective_external_set(
            std::slice::from_ref(&shape),
            std::slice::from_ref(&area),
            &[],
            &[name_regex("^Shape::area$")],
            &[],
            &[],
            &[],
        );
        let area_status = statuses
            .iter()
            .find(|status| status.usr == "c:@S@Shape@F@area#")
            .expect("expected a status for Shape::area");
        assert!(area_status.effective);
    }

    #[test]
    fn a_path_regex_matches_the_declarations_file() {
        let statuses = effective_external_set(
            &[],
            &[free_function(
                "c:@F@f#",
                "f",
                "",
                "/project/input-source/third_party/miniz/miniz.c",
            )],
            &[],
            &[],
            &[path_regex("^/project/input-source/third_party/")],
            &[],
            &[],
        );
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].effective);
    }

    #[test]
    fn manual_exclude_overrides_a_matching_regex_in_both_directions() {
        let usr = "c:@F@f#";
        let statuses = effective_external_set(
            &[],
            &[free_function(usr, "f", "", "/project/third_party/f.cpp")],
            &[],
            &[],
            &[path_regex("^/project/third_party/")],
            &[],
            &[manual_mark(usr, false)],
        );
        assert_eq!(statuses.len(), 1);
        assert!(
            !statuses[0].effective,
            "manual exclude must win over a matching path regex: {:#?}",
            statuses[0]
        );
        assert!(statuses[0].sources.contains(&ExternalSource::PathRegex {
            pattern: "^/project/third_party/".to_owned()
        }));
        assert!(statuses[0].sources.contains(&ExternalSource::ManualExclude));
    }

    #[test]
    fn manual_include_overrides_a_non_matching_regex() {
        let usr = "c:@F@f#";
        let statuses = effective_external_set(
            &[],
            &[free_function(usr, "f", "", "/project/src/f.cpp")],
            &[],
            &[],
            &[path_regex("^/project/third_party/")],
            &[],
            &[manual_mark(usr, true)],
        );
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].effective);
        assert_eq!(statuses[0].sources, vec![ExternalSource::ManualInclude]);
    }

    #[test]
    fn a_free_function_with_no_definition_and_an_incoming_call_is_auto_detected() {
        let mut undefined = free_function("c:@F@undef#", "undef", "", "/project/undef.h");
        undefined.has_definition = false;
        let call = CallEdge {
            caller_usr: "c:@F@caller#".to_owned(),
            resolution: CallResolution::Resolved {
                callee_usr: "c:@F@undef#".to_owned(),
                is_dynamic_dispatch: false,
            },
            file: "/project/caller.cpp".to_owned(),
            line: 3,
            column: 5,
        };
        let statuses = effective_external_set(&[], &[undefined], &[call], &[], &[], &[], &[]);

        let status = statuses
            .iter()
            .find(|status| status.usr == "c:@F@undef#")
            .expect("expected undef to be auto-detected");
        assert!(
            status.effective,
            "auto-detected candidates are active by default"
        );
        assert_eq!(status.sources, vec![ExternalSource::AutoUndefinedFunction]);
    }

    #[test]
    fn a_defined_function_is_never_auto_detected_even_if_called() {
        let call = CallEdge {
            caller_usr: "c:@F@caller#".to_owned(),
            resolution: CallResolution::Resolved {
                callee_usr: "c:@F@f#".to_owned(),
                is_dynamic_dispatch: false,
            },
            file: "/project/caller.cpp".to_owned(),
            line: 3,
            column: 5,
        };
        let statuses = effective_external_set(
            &[],
            &[free_function("c:@F@f#", "f", "", "/project/f.cpp")],
            &[call],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(statuses.is_empty(), "{statuses:#?}");
    }

    #[test]
    fn an_unresolved_call_is_never_auto_detected() {
        let call = CallEdge {
            caller_usr: "c:@F@caller#".to_owned(),
            resolution: CallResolution::Unresolved {
                reason: "call through a function pointer".to_owned(),
            },
            file: "/project/caller.cpp".to_owned(),
            line: 3,
            column: 5,
        };
        let statuses = effective_external_set(&[], &[], &[call], &[], &[], &[], &[]);
        assert!(statuses.is_empty(), "{statuses:#?}");
    }

    fn file_mark(file: &str) -> FileMark {
        FileMark {
            file: file.to_owned(),
            decided_at: "2026-08-19T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn a_file_mark_makes_every_declaration_in_that_file_effective() {
        let file = "/project/input-source/third_party/humlib/humlib.cpp";
        let types = [type_declaration("c:@S@Foo", "Foo", "", file)];
        let functions = [
            free_function("c:@F@bar#", "bar", "", file),
            free_function("c:@F@elsewhere#", "elsewhere", "", "/project/other.cpp"),
        ];

        let statuses =
            effective_external_set(&types, &functions, &[], &[], &[], &[file_mark(file)], &[]);

        let foo = statuses
            .iter()
            .find(|status| status.usr == "c:@S@Foo")
            .expect("expected Foo to be marked external via its file");
        assert!(foo.effective);
        assert_eq!(
            foo.sources,
            vec![ExternalSource::FileMark {
                file: file.to_owned()
            }]
        );

        let bar = statuses
            .iter()
            .find(|status| status.usr == "c:@F@bar#")
            .expect("expected bar to be marked external via its file");
        assert!(bar.effective);

        assert!(
            statuses
                .iter()
                .all(|status| status.usr != "c:@F@elsewhere#"),
            "a declaration outside the marked file must not be affected: {statuses:#?}"
        );
    }

    #[test]
    fn a_manual_exclude_overrides_a_file_mark() {
        let file = "/project/input-source/third_party/humlib/humlib.cpp";
        let usr = "c:@F@bar#";
        let statuses = effective_external_set(
            &[],
            &[free_function(usr, "bar", "", file)],
            &[],
            &[],
            &[],
            &[file_mark(file)],
            &[manual_mark(usr, false)],
        );
        assert_eq!(statuses.len(), 1);
        assert!(
            !statuses[0].effective,
            "manual exclude must win over a file mark: {:#?}",
            statuses[0]
        );
    }

    #[test]
    fn expand_type_mark_collects_the_type_itself_and_every_owned_method() {
        let functions = [
            method("c:@S@Shape@F@area#", "area", "c:@S@Shape", ""),
            method("c:@S@Shape@F@perimeter#", "perimeter", "c:@S@Shape", ""),
            free_function("c:@F@unrelated#", "unrelated", "", "/project/f.cpp"),
        ];

        let usrs = expand_type_mark("c:@S@Shape", &functions);
        assert_eq!(
            usrs,
            vec![
                "c:@S@Shape".to_owned(),
                "c:@S@Shape@F@area#".to_owned(),
                "c:@S@Shape@F@perimeter#".to_owned(),
            ]
        );
    }

    #[test]
    fn validate_regex_pattern_accepts_a_valid_pattern_and_rejects_an_invalid_one() {
        assert!(validate_regex_pattern("^humlib::").is_ok());
        assert!(validate_regex_pattern("(unclosed").is_err());
    }
}
