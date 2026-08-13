use std::collections::HashMap;
use std::fs;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::function_catalog::{
    CallEdge, CallResolution, FunctionDeclaration, FunctionDeclarationKind,
};
use crate::ingest::CompilationUnit;
use crate::ir;
use crate::mapping::MappingDecision;
use crate::source_catalog::{SourceFile, SourceFileKind};
use crate::type_catalog::{
    TypeDeclaration, TypeDeclarationKind, TypeDependency, TypeUsage, TypeUsageKind,
};

use super::PersistenceError;

/// Per-project database, stored inside the project's own directory,
/// holding data specific to that single project.
pub struct ProjectStore {
    connection: Connection,
}

impl ProjectStore {
    pub fn open(path: &Path) -> Result<Self, PersistenceError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS compilation_units (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                directory TEXT NOT NULL,
                file TEXT NOT NULL,
                command TEXT,
                arguments_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS type_declarations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                namespace TEXT NOT NULL,
                file TEXT NOT NULL,
                line INTEGER NOT NULL,
                column INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_column INTEGER NOT NULL,
                usr TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS type_dependencies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                caller_name TEXT NOT NULL,
                caller_kind TEXT NOT NULL,
                caller_namespace TEXT NOT NULL,
                caller_file TEXT NOT NULL,
                caller_line INTEGER NOT NULL,
                caller_column INTEGER NOT NULL,
                caller_end_line INTEGER NOT NULL,
                caller_end_column INTEGER NOT NULL,
                caller_usr TEXT NOT NULL DEFAULT '',
                callee_name TEXT NOT NULL,
                callee_kind TEXT NOT NULL,
                callee_namespace TEXT NOT NULL,
                callee_file TEXT NOT NULL,
                callee_line INTEGER NOT NULL,
                callee_column INTEGER NOT NULL,
                callee_end_line INTEGER NOT NULL,
                callee_end_column INTEGER NOT NULL,
                callee_usr TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS source_files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                kind TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS type_usages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                type_usr TEXT NOT NULL,
                kind TEXT NOT NULL,
                file TEXT NOT NULL,
                line INTEGER NOT NULL,
                column INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_type_usages_type_usr ON type_usages(type_usr);
            CREATE TABLE IF NOT EXISTS function_declarations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                namespace TEXT NOT NULL,
                owning_class_usr TEXT,
                signature TEXT NOT NULL,
                file TEXT NOT NULL,
                line INTEGER NOT NULL,
                column INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                end_column INTEGER NOT NULL,
                usr TEXT NOT NULL DEFAULT '',
                is_virtual INTEGER NOT NULL,
                is_pure_virtual INTEGER NOT NULL DEFAULT 0,
                is_defaulted INTEGER NOT NULL DEFAULT 0,
                overridden_usrs_json TEXT NOT NULL DEFAULT '[]'
            );
            CREATE TABLE IF NOT EXISTS call_edges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                caller_usr TEXT NOT NULL,
                callee_usr TEXT,
                is_dynamic_dispatch INTEGER,
                unresolved_reason TEXT,
                file TEXT NOT NULL,
                line INTEGER NOT NULL,
                column INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_call_edges_caller_usr ON call_edges(caller_usr);
            CREATE INDEX IF NOT EXISTS idx_call_edges_callee_usr ON call_edges(callee_usr);
            CREATE TABLE IF NOT EXISTS type_mappings (
                type_usr TEXT PRIMARY KEY,
                option_id TEXT NOT NULL,
                decided_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ir_functions (
                usr TEXT PRIMARY KEY,
                data TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ir_records (
                usr TEXT PRIMARY KEY,
                data TEXT NOT NULL
            );",
        )?;

        migrate_type_columns(&connection)?;
        migrate_function_columns(&connection)?;

        Ok(Self { connection })
    }

    /// Replaces the full set of compilation units with the ones from the
    /// latest ingest, since they describe the current state of the build
    /// rather than an append-only history.
    pub fn replace_compilation_units(
        &mut self,
        units: &[CompilationUnit],
    ) -> Result<(), PersistenceError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM compilation_units", [])?;

        for unit in units {
            let arguments_json = serde_json::to_string(&unit.arguments)?;
            transaction.execute(
                "INSERT INTO compilation_units (directory, file, command, arguments_json)
                 VALUES (?1, ?2, ?3, ?4)",
                params![unit.directory, unit.file, unit.command, arguments_json],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn list_compilation_units(&self) -> Result<Vec<CompilationUnit>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT directory, file, command, arguments_json FROM compilation_units ORDER BY id",
        )?;

        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut units = Vec::new();
        for row in rows {
            let (directory, file, command, arguments_json) = row?;
            let arguments: Vec<String> = serde_json::from_str(&arguments_json)?;
            units.push(CompilationUnit {
                directory,
                file,
                command,
                arguments,
            });
        }

        Ok(units)
    }

    /// Replaces the full set of cataloged type declarations with the ones
    /// from the latest `libclang` extraction, since they describe the
    /// current state of the source tree rather than an append-only history.
    ///
    /// `type_mappings` is *not* wholesale-replaced the same way — it holds a
    /// user decision, not derived state, so wiping it on every re-extraction
    /// would silently discard a choice the user made. But a decision whose
    /// `type_usr` no longer names anything in the fresh catalog (the type
    /// was renamed or removed, so `libclang` now assigns it a different USR
    /// or none at all) is orphaned, pointing at nothing — pruned here, in
    /// the same transaction, once the new catalog is in place.
    pub fn replace_type_declarations(
        &mut self,
        declarations: &[TypeDeclaration],
    ) -> Result<(), PersistenceError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM type_declarations", [])?;

        for declaration in declarations {
            transaction.execute(
                "INSERT INTO type_declarations (name, kind, namespace, file, line, column, end_line, end_column, usr)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    declaration.name,
                    declaration.kind.as_str(),
                    declaration.namespace,
                    declaration.file,
                    declaration.line,
                    declaration.column,
                    declaration.end_line,
                    declaration.end_column,
                    declaration.usr
                ],
            )?;
        }

        transaction.execute(
            "DELETE FROM type_mappings WHERE type_usr NOT IN (SELECT usr FROM type_declarations)",
            [],
        )?;

        transaction.commit()?;
        Ok(())
    }

    pub fn list_type_declarations(&self) -> Result<Vec<TypeDeclaration>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT name, kind, namespace, file, line, column, end_line, end_column, usr
             FROM type_declarations ORDER BY id",
        )?;

        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u32>(4)?,
                row.get::<_, u32>(5)?,
                row.get::<_, u32>(6)?,
                row.get::<_, u32>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?;

        let mut declarations = Vec::new();
        for row in rows {
            let (name, kind, namespace, file, line, column, end_line, end_column, usr) = row?;
            let Some(kind) = TypeDeclarationKind::parse(&kind) else {
                continue;
            };

            declarations.push(TypeDeclaration {
                name,
                kind,
                namespace,
                file,
                line,
                column,
                end_line,
                end_column,
                usr,
            });
        }

        Ok(declarations)
    }

    /// Replaces the full set of type dependency edges with the ones from the
    /// latest `libclang` extraction, since they describe the current state
    /// of the source tree rather than an append-only history.
    ///
    /// Stores each side of the edge denormalized (name/kind/file/line/column)
    /// rather than as a foreign key into `type_declarations`, since there is
    /// no stable id for a `TypeDeclaration` shared between the in-memory
    /// catalog and the database rows.
    pub fn replace_type_dependencies(
        &mut self,
        dependencies: &[TypeDependency],
    ) -> Result<(), PersistenceError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM type_dependencies", [])?;

        for dependency in dependencies {
            transaction.execute(
                "INSERT INTO type_dependencies (
                    caller_name, caller_kind, caller_namespace, caller_file, caller_line, caller_column, caller_end_line, caller_end_column, caller_usr,
                    callee_name, callee_kind, callee_namespace, callee_file, callee_line, callee_column, callee_end_line, callee_end_column, callee_usr
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                params![
                    dependency.caller.name,
                    dependency.caller.kind.as_str(),
                    dependency.caller.namespace,
                    dependency.caller.file,
                    dependency.caller.line,
                    dependency.caller.column,
                    dependency.caller.end_line,
                    dependency.caller.end_column,
                    dependency.caller.usr,
                    dependency.callee.name,
                    dependency.callee.kind.as_str(),
                    dependency.callee.namespace,
                    dependency.callee.file,
                    dependency.callee.line,
                    dependency.callee.column,
                    dependency.callee.end_line,
                    dependency.callee.end_column,
                    dependency.callee.usr,
                ],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn list_type_dependencies(&self) -> Result<Vec<TypeDependency>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT caller_name, caller_kind, caller_namespace, caller_file, caller_line, caller_column, caller_end_line, caller_end_column, caller_usr,
                    callee_name, callee_kind, callee_namespace, callee_file, callee_line, callee_column, callee_end_line, callee_end_column, callee_usr
             FROM type_dependencies ORDER BY id",
        )?;

        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u32>(4)?,
                row.get::<_, u32>(5)?,
                row.get::<_, u32>(6)?,
                row.get::<_, u32>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, String>(12)?,
                row.get::<_, u32>(13)?,
                row.get::<_, u32>(14)?,
                row.get::<_, u32>(15)?,
                row.get::<_, u32>(16)?,
                row.get::<_, String>(17)?,
            ))
        })?;

        let mut dependencies = Vec::new();
        for row in rows {
            let (
                caller_name,
                caller_kind,
                caller_namespace,
                caller_file,
                caller_line,
                caller_column,
                caller_end_line,
                caller_end_column,
                caller_usr,
                callee_name,
                callee_kind,
                callee_namespace,
                callee_file,
                callee_line,
                callee_column,
                callee_end_line,
                callee_end_column,
                callee_usr,
            ) = row?;

            let (Some(caller_kind), Some(callee_kind)) = (
                TypeDeclarationKind::parse(&caller_kind),
                TypeDeclarationKind::parse(&callee_kind),
            ) else {
                continue;
            };

            dependencies.push(TypeDependency {
                caller: TypeDeclaration {
                    name: caller_name,
                    kind: caller_kind,
                    namespace: caller_namespace,
                    file: caller_file,
                    line: caller_line,
                    column: caller_column,
                    end_line: caller_end_line,
                    end_column: caller_end_column,
                    usr: caller_usr,
                },
                callee: TypeDeclaration {
                    name: callee_name,
                    kind: callee_kind,
                    namespace: callee_namespace,
                    file: callee_file,
                    line: callee_line,
                    column: callee_column,
                    end_line: callee_end_line,
                    end_column: callee_end_column,
                    usr: callee_usr,
                },
            });
        }

        Ok(dependencies)
    }

    /// Replaces the full set of discovered source files with the ones from
    /// the latest `libclang` extraction, since they describe the current
    /// state of the source tree rather than an append-only history.
    pub fn replace_source_files(&mut self, files: &[SourceFile]) -> Result<(), PersistenceError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM source_files", [])?;

        for file in files {
            transaction.execute(
                "INSERT INTO source_files (path, kind) VALUES (?1, ?2)",
                params![file.path, file.kind.as_str()],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn list_source_files(&self) -> Result<Vec<SourceFile>, PersistenceError> {
        let mut statement = self
            .connection
            .prepare("SELECT path, kind FROM source_files ORDER BY id")?;

        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut files = Vec::new();
        for row in rows {
            let (path, kind) = row?;
            let Some(kind) = SourceFileKind::parse(&kind) else {
                continue;
            };

            files.push(SourceFile { path, kind });
        }

        Ok(files)
    }

    /// Replaces the full set of type usage occurrences (US-4) with the ones
    /// from the latest `libclang` extraction, since they describe the
    /// current state of the source tree rather than an append-only history.
    pub fn replace_type_usages(&mut self, usages: &[TypeUsage]) -> Result<(), PersistenceError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM type_usages", [])?;

        for usage in usages {
            transaction.execute(
                "INSERT INTO type_usages (type_usr, kind, file, line, column)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    usage.type_usr,
                    usage.kind.as_str(),
                    usage.file,
                    usage.line,
                    usage.column
                ],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    /// Every recorded usage of the type identified by `type_usr`, for the
    /// "click a type, see every place it's used" navigation US-4 asks for —
    /// answered straight from the persisted index, no reparsing.
    pub fn list_type_usages_for(&self, type_usr: &str) -> Result<Vec<TypeUsage>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT type_usr, kind, file, line, column
             FROM type_usages WHERE type_usr = ?1 ORDER BY file, line, column",
        )?;

        let rows = statement.query_map(params![type_usr], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, u32>(4)?,
            ))
        })?;

        let mut usages = Vec::new();
        for row in rows {
            let (type_usr, kind, file, line, column) = row?;
            let Some(kind) = TypeUsageKind::parse(&kind) else {
                continue;
            };

            usages.push(TypeUsage {
                type_usr,
                kind,
                file,
                line,
                column,
            });
        }

        Ok(usages)
    }

    /// The number of recorded usages per type, keyed by `usr` — what the
    /// type list shows as its "N usages" column (US-4) without a per-row
    /// query.
    pub fn type_usage_counts(&self) -> Result<HashMap<String, usize>, PersistenceError> {
        let mut statement = self
            .connection
            .prepare("SELECT type_usr, COUNT(*) FROM type_usages GROUP BY type_usr")?;

        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        let mut counts = HashMap::new();
        for row in rows {
            let (type_usr, count) = row?;
            counts.insert(type_usr, count as usize);
        }

        Ok(counts)
    }

    /// Replaces the full set of cataloged function/method/macro declarations
    /// (US-5) with the ones from the latest `libclang` extraction, mirroring
    /// `replace_type_declarations`.
    pub fn replace_function_declarations(
        &mut self,
        declarations: &[FunctionDeclaration],
    ) -> Result<(), PersistenceError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM function_declarations", [])?;

        for declaration in declarations {
            let overridden_usrs_json = serde_json::to_string(&declaration.overridden_usrs)?;
            transaction.execute(
                "INSERT INTO function_declarations (
                    name, kind, namespace, owning_class_usr, signature, file, line, column,
                    end_line, end_column, usr, is_virtual, is_pure_virtual, is_defaulted,
                    overridden_usrs_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    declaration.name,
                    declaration.kind.as_str(),
                    declaration.namespace,
                    declaration.owning_class_usr,
                    declaration.signature,
                    declaration.file,
                    declaration.line,
                    declaration.column,
                    declaration.end_line,
                    declaration.end_column,
                    declaration.usr,
                    declaration.is_virtual,
                    declaration.is_pure_virtual,
                    declaration.is_defaulted,
                    overridden_usrs_json,
                ],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn list_function_declarations(&self) -> Result<Vec<FunctionDeclaration>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT name, kind, namespace, owning_class_usr, signature, file, line, column,
                    end_line, end_column, usr, is_virtual, is_pure_virtual, is_defaulted,
                    overridden_usrs_json
             FROM function_declarations ORDER BY id",
        )?;

        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, u32>(6)?,
                row.get::<_, u32>(7)?,
                row.get::<_, u32>(8)?,
                row.get::<_, u32>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, bool>(11)?,
                row.get::<_, bool>(12)?,
                row.get::<_, bool>(13)?,
                row.get::<_, String>(14)?,
            ))
        })?;

        let mut declarations = Vec::new();
        for row in rows {
            let (
                name,
                kind,
                namespace,
                owning_class_usr,
                signature,
                file,
                line,
                column,
                end_line,
                end_column,
                usr,
                is_virtual,
                is_pure_virtual,
                is_defaulted,
                overridden_usrs_json,
            ) = row?;
            let Some(kind) = FunctionDeclarationKind::parse(&kind) else {
                continue;
            };
            let overridden_usrs: Vec<String> = serde_json::from_str(&overridden_usrs_json)?;

            declarations.push(FunctionDeclaration {
                name,
                kind,
                namespace,
                owning_class_usr,
                signature,
                file,
                line,
                column,
                end_line,
                end_column,
                usr,
                is_virtual,
                is_pure_virtual,
                is_defaulted,
                overridden_usrs,
            });
        }

        Ok(declarations)
    }

    /// Replaces the full set of call edges (US-5) with the ones from the
    /// latest `libclang` extraction, mirroring `replace_type_usages`.
    pub fn replace_call_edges(&mut self, calls: &[CallEdge]) -> Result<(), PersistenceError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM call_edges", [])?;

        for call in calls {
            let (callee_usr, is_dynamic_dispatch, unresolved_reason) = match &call.resolution {
                CallResolution::Resolved {
                    callee_usr,
                    is_dynamic_dispatch,
                } => (Some(callee_usr.clone()), Some(*is_dynamic_dispatch), None),
                CallResolution::Unresolved { reason } => (None, None, Some(reason.clone())),
            };

            transaction.execute(
                "INSERT INTO call_edges (
                    caller_usr, callee_usr, is_dynamic_dispatch, unresolved_reason, file, line, column
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    call.caller_usr,
                    callee_usr,
                    is_dynamic_dispatch,
                    unresolved_reason,
                    call.file,
                    call.line,
                    call.column,
                ],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    /// Every recorded call whose target resolves to `callee_usr` — US-5
    /// criterion 5's "from a definition, list its callers" — answered from
    /// the persisted index, no reparsing.
    pub fn list_callers_for(&self, callee_usr: &str) -> Result<Vec<CallEdge>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT caller_usr, callee_usr, is_dynamic_dispatch, unresolved_reason, file, line, column
             FROM call_edges WHERE callee_usr = ?1 ORDER BY file, line, column",
        )?;

        let rows = statement.query_map(params![callee_usr], row_to_call_edge)?;

        let mut calls = Vec::new();
        for row in rows {
            calls.push(row?);
        }

        Ok(calls)
    }

    /// Every recorded call site within `file` — the flip side of
    /// `list_callers_for`: instead of "who calls this function", "what does
    /// this file, already open in the source viewer, call". Answers US-5
    /// criterion 5's other direction (click a call in open source, jump to
    /// its definition) from the persisted index, no reparsing.
    pub fn list_calls_in_file(&self, file: &str) -> Result<Vec<CallEdge>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT caller_usr, callee_usr, is_dynamic_dispatch, unresolved_reason, file, line, column
             FROM call_edges WHERE file = ?1 ORDER BY line, column",
        )?;

        let rows = statement.query_map(params![file], row_to_call_edge)?;

        let mut calls = Vec::new();
        for row in rows {
            calls.push(row?);
        }

        Ok(calls)
    }

    /// The number of recorded callers per function, keyed by `usr` — mirrors
    /// `type_usage_counts`, what a function list shows as its "N callers"
    /// column without a per-row query. Unresolved calls (US-5 criterion 6)
    /// have no `callee_usr` and so contribute to no function's count.
    pub fn call_counts(&self) -> Result<HashMap<String, usize>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT callee_usr, COUNT(*) FROM call_edges
             WHERE callee_usr IS NOT NULL GROUP BY callee_usr",
        )?;

        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        let mut counts = HashMap::new();
        for row in rows {
            let (callee_usr, count) = row?;
            counts.insert(callee_usr, count as usize);
        }

        Ok(counts)
    }

    /// Records (or updates) the chosen mapping option for one type — keyed
    /// by `type_usr` (US-3's stable identity). Unlike every other table in
    /// this store, `type_mappings` isn't wholesale-replaced on
    /// re-extraction: it holds user decisions, not derived catalog data, so
    /// an upsert per type is what makes "reabrir o projeto preserva a
    /// decisão gravada" (US-7 criterion 4) true without any extra plumbing —
    /// this table is simply never cleared by anything else in this store.
    pub fn set_type_mapping(&mut self, decision: &MappingDecision) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO type_mappings (type_usr, option_id, decided_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(type_usr) DO UPDATE SET
                 option_id = excluded.option_id,
                 decided_at = excluded.decided_at",
            params![decision.type_usr, decision.option_id, decision.decided_at],
        )?;
        Ok(())
    }

    pub fn list_type_mappings(&self) -> Result<Vec<MappingDecision>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT type_usr, option_id, decided_at FROM type_mappings ORDER BY type_usr",
        )?;

        let rows = statement.query_map([], |row| {
            Ok(MappingDecision {
                type_usr: row.get(0)?,
                option_id: row.get(1)?,
                decided_at: row.get(2)?,
            })
        })?;

        let mut decisions = Vec::new();
        for row in rows {
            decisions.push(row?);
        }

        Ok(decisions)
    }

    /// Persists the IR `lower::cpp` produced alongside the
    /// declarations/calls stored by [`Self::replace_function_declarations`]/
    /// [`Self::replace_call_edges`] — reused by `project_service::transpile_project`
    /// so transpiling doesn't reparse every compilation unit with `libclang`
    /// on every request; only project creation (or a future reingest) needs
    /// to re-derive it. Wholesale DELETE+replace like every other catalog
    /// table here: this is derived state, not a user decision (unlike
    /// `type_mappings`, deliberately never cleared by this method).
    pub fn replace_ir(
        &mut self,
        functions: &[ir::Function],
        records: &[ir::Record],
    ) -> Result<(), PersistenceError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM ir_functions", [])?;
        transaction.execute("DELETE FROM ir_records", [])?;

        for function in functions {
            let data = serde_json::to_string(function)?;
            transaction.execute(
                "INSERT INTO ir_functions (usr, data) VALUES (?1, ?2)",
                params![function.usr, data],
            )?;
        }
        for record in records {
            let data = serde_json::to_string(record)?;
            transaction.execute(
                "INSERT INTO ir_records (usr, data) VALUES (?1, ?2)",
                params![record.usr, data],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn list_ir(&self) -> Result<(Vec<ir::Function>, Vec<ir::Record>), PersistenceError> {
        let mut functions_statement = self
            .connection
            .prepare("SELECT data FROM ir_functions ORDER BY usr")?;
        let function_rows = functions_statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut functions = Vec::new();
        for row in function_rows {
            functions.push(serde_json::from_str(&row?)?);
        }

        let mut records_statement = self
            .connection
            .prepare("SELECT data FROM ir_records ORDER BY usr")?;
        let record_rows = records_statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut records = Vec::new();
        for row in record_rows {
            records.push(serde_json::from_str(&row?)?);
        }

        Ok((functions, records))
    }
}

fn row_to_call_edge(row: &rusqlite::Row<'_>) -> rusqlite::Result<CallEdge> {
    let caller_usr: String = row.get(0)?;
    let callee_usr: Option<String> = row.get(1)?;
    let is_dynamic_dispatch: Option<bool> = row.get(2)?;
    let unresolved_reason: Option<String> = row.get(3)?;
    let file: String = row.get(4)?;
    let line: u32 = row.get(5)?;
    let column: u32 = row.get(6)?;

    let resolution = match callee_usr {
        Some(callee_usr) => CallResolution::Resolved {
            callee_usr,
            is_dynamic_dispatch: is_dynamic_dispatch.unwrap_or(false),
        },
        None => CallResolution::Unresolved {
            reason: unresolved_reason.unwrap_or_default(),
        },
    };

    Ok(CallEdge {
        caller_usr,
        resolution,
        file,
        line,
        column,
    })
}

/// Adds the `namespace`/`end_line`/`end_column` columns to `type_declarations`
/// and their `caller_`/`callee_` counterparts to `type_dependencies` when
/// opening a `project.db` written before those columns existed.
///
/// `CREATE TABLE IF NOT EXISTS` above leaves an already-created table
/// untouched, so without this, reopening an older project fails with
/// "no such column" the first time a type is listed. This is a narrow,
/// additive migration rather than a general schema-versioning mechanism (see
/// `docs/plans/User Steps.md`'s open item on that).
fn migrate_type_columns(connection: &Connection) -> Result<(), PersistenceError> {
    ensure_column(
        connection,
        "type_declarations",
        "namespace",
        "namespace TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        connection,
        "type_declarations",
        "end_line",
        "end_line INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "type_declarations",
        "end_column",
        "end_column INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "type_declarations",
        "usr",
        "usr TEXT NOT NULL DEFAULT ''",
    )?;

    for side in ["caller", "callee"] {
        ensure_column(
            connection,
            "type_dependencies",
            &format!("{side}_namespace"),
            &format!("{side}_namespace TEXT NOT NULL DEFAULT ''"),
        )?;
        ensure_column(
            connection,
            "type_dependencies",
            &format!("{side}_end_line"),
            &format!("{side}_end_line INTEGER NOT NULL DEFAULT 0"),
        )?;
        ensure_column(
            connection,
            "type_dependencies",
            &format!("{side}_end_column"),
            &format!("{side}_end_column INTEGER NOT NULL DEFAULT 0"),
        )?;
        ensure_column(
            connection,
            "type_dependencies",
            &format!("{side}_usr"),
            &format!("{side}_usr TEXT NOT NULL DEFAULT ''"),
        )?;
    }

    Ok(())
}

/// A project database created before `overridden_usrs_json` replaced the
/// single-base `overrides_usr` column (multiple-inheritance support in
/// US-5) is missing the new column — same pattern as
/// `migrate_type_columns`. The old `overrides_usr` column, if present, is
/// left in place unused rather than dropped: SQLite's `DROP COLUMN` support
/// is recent enough to not be worth relying on, and an orphaned column is
/// harmless.
fn migrate_function_columns(connection: &Connection) -> Result<(), PersistenceError> {
    ensure_column(
        connection,
        "function_declarations",
        "overridden_usrs_json",
        "overridden_usrs_json TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        connection,
        "function_declarations",
        "is_pure_virtual",
        "is_pure_virtual INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "function_declarations",
        "is_defaulted",
        "is_defaulted INTEGER NOT NULL DEFAULT 0",
    )
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    column_definition: &str,
) -> Result<(), PersistenceError> {
    if !table_has_column(connection, table, column)? {
        connection.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column_definition}"
        ))?;
    }

    Ok(())
}

fn table_has_column(
    connection: &Connection,
    table: &str,
    column: &str,
) -> Result<bool, PersistenceError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = statement.query([])?;

    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::test_support::temp_db_path;

    fn sample_units() -> Vec<CompilationUnit> {
        vec![
            CompilationUnit {
                directory: "/workspace/build".to_owned(),
                file: "/workspace/src/main.cpp".to_owned(),
                command: Some("clang++ -c main.cpp".to_owned()),
                arguments: vec!["clang++".to_owned(), "-c".to_owned(), "main.cpp".to_owned()],
            },
            CompilationUnit {
                directory: "/workspace/build".to_owned(),
                file: "/workspace/src/util.cpp".to_owned(),
                command: None,
                arguments: vec![],
            },
        ]
    }

    #[test]
    fn round_trips_compilation_units() {
        let db_path = temp_db_path("project-units");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_compilation_units(&sample_units())
            .expect("persist compilation units");

        let units = store
            .list_compilation_units()
            .expect("list compilation units");
        assert_eq!(units, sample_units());

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn replacing_compilation_units_clears_previous_entries() {
        let db_path = temp_db_path("project-units-replace");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_compilation_units(&sample_units())
            .expect("persist compilation units");
        store
            .replace_compilation_units(&[])
            .expect("clear compilation units");

        let units = store
            .list_compilation_units()
            .expect("list compilation units");
        assert!(units.is_empty(), "expected no compilation units: {units:?}");

        let _ = fs::remove_file(&db_path);
    }

    fn sample_type_declarations() -> Vec<TypeDeclaration> {
        vec![
            TypeDeclaration {
                name: "Point".to_owned(),
                kind: TypeDeclarationKind::Struct,
                namespace: "geometry".to_owned(),
                file: "/workspace/src/types.h".to_owned(),
                line: 3,
                column: 8,
                end_line: 6,
                end_column: 1,
                usr: "c:@N@geometry@S@Point".to_owned(),
            },
            TypeDeclaration {
                name: "ANSWER".to_owned(),
                kind: TypeDeclarationKind::ConstantMacro,
                namespace: String::new(),
                file: "/workspace/src/types.h".to_owned(),
                line: 1,
                column: 9,
                end_line: 1,
                end_column: 20,
                usr: "c:@macro@ANSWER".to_owned(),
            },
        ]
    }

    /// Reproduces reopening a `project.db` written before `namespace` and
    /// the extent columns existed: `CREATE TABLE IF NOT EXISTS` leaves an
    /// already-created table untouched, so `ProjectStore::open` must migrate
    /// it explicitly instead of relying on that statement.
    #[test]
    fn opening_a_pre_namespace_database_adds_the_missing_columns() {
        let db_path = temp_db_path("project-types-pre-namespace-migration");
        {
            let connection = Connection::open(&db_path).expect("create legacy database");
            connection
                .execute_batch(
                    "CREATE TABLE type_declarations (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        name TEXT NOT NULL,
                        kind TEXT NOT NULL,
                        file TEXT NOT NULL,
                        line INTEGER NOT NULL,
                        column INTEGER NOT NULL
                    );
                    CREATE TABLE type_dependencies (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        caller_name TEXT NOT NULL,
                        caller_kind TEXT NOT NULL,
                        caller_file TEXT NOT NULL,
                        caller_line INTEGER NOT NULL,
                        caller_column INTEGER NOT NULL,
                        callee_name TEXT NOT NULL,
                        callee_kind TEXT NOT NULL,
                        callee_file TEXT NOT NULL,
                        callee_line INTEGER NOT NULL,
                        callee_column INTEGER NOT NULL
                    );",
                )
                .expect("create legacy tables");
            connection
                .execute(
                    "INSERT INTO type_declarations (name, kind, file, line, column)
                     VALUES ('Point', 'struct', '/workspace/src/types.h', 3, 8)",
                    [],
                )
                .expect("insert legacy row");
        }

        let store = ProjectStore::open(&db_path).expect("open and migrate legacy database");
        let declarations = store
            .list_type_declarations()
            .expect("list type declarations after migration");

        assert_eq!(
            declarations,
            vec![TypeDeclaration {
                name: "Point".to_owned(),
                kind: TypeDeclarationKind::Struct,
                namespace: String::new(),
                file: "/workspace/src/types.h".to_owned(),
                line: 3,
                column: 8,
                end_line: 0,
                end_column: 0,
                usr: String::new(),
            }]
        );

        let _ = fs::remove_file(&db_path);
    }

    /// Multiple-inheritance support (US-5) replaced the single-base
    /// `overrides_usr` column with `overridden_usrs_json`; a database
    /// created before that change needs the new column added, mirroring
    /// `opening_a_pre_namespace_database_adds_the_missing_columns` above.
    #[test]
    fn opening_a_pre_multiple_inheritance_database_adds_the_missing_column() {
        let db_path = temp_db_path("project-functions-pre-multiple-inheritance-migration");
        {
            let connection = Connection::open(&db_path).expect("create legacy database");
            connection
                .execute_batch(
                    "CREATE TABLE function_declarations (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        name TEXT NOT NULL,
                        kind TEXT NOT NULL,
                        namespace TEXT NOT NULL,
                        owning_class_usr TEXT,
                        signature TEXT NOT NULL,
                        file TEXT NOT NULL,
                        line INTEGER NOT NULL,
                        column INTEGER NOT NULL,
                        end_line INTEGER NOT NULL,
                        end_column INTEGER NOT NULL,
                        usr TEXT NOT NULL DEFAULT '',
                        is_virtual INTEGER NOT NULL,
                        overrides_usr TEXT
                    );",
                )
                .expect("create legacy table");
            connection
                .execute(
                    "INSERT INTO function_declarations (
                        name, kind, namespace, owning_class_usr, signature, file, line, column,
                        end_line, end_column, usr, is_virtual, overrides_usr
                     ) VALUES ('area', 'method', 'geometry', 'c:@N@geometry@S@Shape',
                        'double geometry::Shape::area() const', '/workspace/src/shapes.h',
                        4, 19, 4, 40, 'c:@N@geometry@S@Shape@F@area#1#', 1,
                        'c:@N@geometry@S@Drawable@F@area#1#')",
                    [],
                )
                .expect("insert legacy row");
        }

        let store = ProjectStore::open(&db_path).expect("open and migrate legacy database");
        let declarations = store
            .list_function_declarations()
            .expect("list function declarations after migration");

        // The migration only adds the new column with its default (`[]`) —
        // it does not backfill data from the old `overrides_usr` column,
        // same "aceitável por ora" stance as the rest of this migration
        // family. What matters here is that opening the legacy database
        // doesn't fail and the new column reads back as valid, empty JSON.
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].name, "area");
        assert_eq!(declarations[0].overridden_usrs, Vec::<String>::new());

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn round_trips_type_declarations() {
        let db_path = temp_db_path("project-types");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_type_declarations(&sample_type_declarations())
            .expect("persist type declarations");

        let declarations = store
            .list_type_declarations()
            .expect("list type declarations");
        assert_eq!(declarations, sample_type_declarations());

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn replacing_type_declarations_clears_previous_entries() {
        let db_path = temp_db_path("project-types-replace");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_type_declarations(&sample_type_declarations())
            .expect("persist type declarations");
        store
            .replace_type_declarations(&[])
            .expect("clear type declarations");

        let declarations = store
            .list_type_declarations()
            .expect("list type declarations");
        assert!(
            declarations.is_empty(),
            "expected no type declarations: {declarations:?}"
        );

        let _ = fs::remove_file(&db_path);
    }

    fn sample_type_dependencies() -> Vec<TypeDependency> {
        let point = TypeDeclaration {
            name: "Point".to_owned(),
            kind: TypeDeclarationKind::Struct,
            namespace: "geometry".to_owned(),
            file: "/workspace/src/types.h".to_owned(),
            line: 3,
            column: 8,
            end_line: 6,
            end_column: 1,
            usr: "c:@N@geometry@S@Point".to_owned(),
        };
        let rect = TypeDeclaration {
            name: "Rect".to_owned(),
            kind: TypeDeclarationKind::Struct,
            namespace: "geometry".to_owned(),
            file: "/workspace/src/types.h".to_owned(),
            line: 8,
            column: 8,
            end_line: 11,
            end_column: 1,
            usr: "c:@N@geometry@S@Rect".to_owned(),
        };

        vec![TypeDependency {
            caller: rect,
            callee: point,
        }]
    }

    #[test]
    fn round_trips_type_dependencies() {
        let db_path = temp_db_path("project-type-deps");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_type_dependencies(&sample_type_dependencies())
            .expect("persist type dependencies");

        let dependencies = store
            .list_type_dependencies()
            .expect("list type dependencies");
        assert_eq!(dependencies, sample_type_dependencies());

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn replacing_type_dependencies_clears_previous_entries() {
        let db_path = temp_db_path("project-type-deps-replace");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_type_dependencies(&sample_type_dependencies())
            .expect("persist type dependencies");
        store
            .replace_type_dependencies(&[])
            .expect("clear type dependencies");

        let dependencies = store
            .list_type_dependencies()
            .expect("list type dependencies");
        assert!(
            dependencies.is_empty(),
            "expected no type dependencies: {dependencies:?}"
        );

        let _ = fs::remove_file(&db_path);
    }

    fn sample_source_files() -> Vec<SourceFile> {
        vec![
            SourceFile {
                path: "/workspace/src/main.cpp".to_owned(),
                kind: SourceFileKind::TranslationUnit,
            },
            SourceFile {
                path: "/workspace/src/types.h".to_owned(),
                kind: SourceFileKind::Header,
            },
        ]
    }

    #[test]
    fn round_trips_source_files() {
        let db_path = temp_db_path("project-source-files");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_source_files(&sample_source_files())
            .expect("persist source files");

        let files = store.list_source_files().expect("list source files");
        assert_eq!(files, sample_source_files());

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn replacing_source_files_clears_previous_entries() {
        let db_path = temp_db_path("project-source-files-replace");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_source_files(&sample_source_files())
            .expect("persist source files");
        store.replace_source_files(&[]).expect("clear source files");

        let files = store.list_source_files().expect("list source files");
        assert!(files.is_empty(), "expected no source files: {files:?}");

        let _ = fs::remove_file(&db_path);
    }

    fn sample_type_usages() -> Vec<TypeUsage> {
        vec![
            TypeUsage {
                type_usr: "c:@N@geometry@S@Point".to_owned(),
                kind: TypeUsageKind::Field,
                file: "/workspace/src/types.h".to_owned(),
                line: 9,
                column: 11,
            },
            TypeUsage {
                type_usr: "c:@N@geometry@S@Point".to_owned(),
                kind: TypeUsageKind::Parameter,
                file: "/workspace/src/main.cpp".to_owned(),
                line: 4,
                column: 22,
            },
            TypeUsage {
                type_usr: "c:@N@geometry@S@Widget".to_owned(),
                kind: TypeUsageKind::Inheritance,
                file: "/workspace/src/types.h".to_owned(),
                line: 8,
                column: 20,
            },
        ]
    }

    #[test]
    fn round_trips_type_usages() {
        let db_path = temp_db_path("project-type-usages");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_type_usages(&sample_type_usages())
            .expect("persist type usages");

        let point_usages = store
            .list_type_usages_for("c:@N@geometry@S@Point")
            .expect("list usages for Point");
        assert_eq!(
            point_usages,
            vec![
                TypeUsage {
                    type_usr: "c:@N@geometry@S@Point".to_owned(),
                    kind: TypeUsageKind::Parameter,
                    file: "/workspace/src/main.cpp".to_owned(),
                    line: 4,
                    column: 22,
                },
                TypeUsage {
                    type_usr: "c:@N@geometry@S@Point".to_owned(),
                    kind: TypeUsageKind::Field,
                    file: "/workspace/src/types.h".to_owned(),
                    line: 9,
                    column: 11,
                },
            ]
        );

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn replacing_type_usages_clears_previous_entries() {
        let db_path = temp_db_path("project-type-usages-replace");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_type_usages(&sample_type_usages())
            .expect("persist type usages");
        store.replace_type_usages(&[]).expect("clear type usages");

        let usages = store
            .list_type_usages_for("c:@N@geometry@S@Point")
            .expect("list usages for Point");
        assert!(usages.is_empty(), "expected no usages: {usages:?}");

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn counts_usages_per_type() {
        let db_path = temp_db_path("project-type-usage-counts");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_type_usages(&sample_type_usages())
            .expect("persist type usages");

        let counts = store.type_usage_counts().expect("compute usage counts");
        assert_eq!(counts.get("c:@N@geometry@S@Point"), Some(&2));
        assert_eq!(counts.get("c:@N@geometry@S@Widget"), Some(&1));
        assert_eq!(counts.len(), 2);

        let _ = fs::remove_file(&db_path);
    }

    fn sample_function_declarations() -> Vec<FunctionDeclaration> {
        vec![
            FunctionDeclaration {
                name: "area".to_owned(),
                kind: FunctionDeclarationKind::Method,
                namespace: "geometry".to_owned(),
                owning_class_usr: Some("c:@N@geometry@S@Shape".to_owned()),
                signature: "double geometry::Shape::area() const".to_owned(),
                file: "/workspace/src/shapes.h".to_owned(),
                line: 4,
                column: 19,
                end_line: 4,
                end_column: 40,
                usr: "c:@N@geometry@S@Shape@F@area#1#".to_owned(),
                is_virtual: true,
                is_pure_virtual: false,
                is_defaulted: false,
                overridden_usrs: vec![
                    "c:@N@geometry@S@Drawable@F@area#1#".to_owned(),
                    "c:@N@geometry@S@Measurable@F@area#1#".to_owned(),
                ],
            },
            FunctionDeclaration {
                name: "add".to_owned(),
                kind: FunctionDeclarationKind::FreeFunction,
                namespace: String::new(),
                owning_class_usr: None,
                signature: "int add(int a, int b)".to_owned(),
                file: "/workspace/src/math.cpp".to_owned(),
                line: 1,
                column: 5,
                end_line: 3,
                end_column: 1,
                usr: "c:@F@add#I#I#".to_owned(),
                is_virtual: false,
                is_pure_virtual: false,
                is_defaulted: false,
                overridden_usrs: Vec::new(),
            },
        ]
    }

    #[test]
    fn round_trips_function_declarations() {
        let db_path = temp_db_path("project-functions");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_function_declarations(&sample_function_declarations())
            .expect("persist function declarations");

        let declarations = store
            .list_function_declarations()
            .expect("list function declarations");
        assert_eq!(declarations, sample_function_declarations());

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn replacing_function_declarations_clears_previous_entries() {
        let db_path = temp_db_path("project-functions-replace");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_function_declarations(&sample_function_declarations())
            .expect("persist function declarations");
        store
            .replace_function_declarations(&[])
            .expect("clear function declarations");

        let declarations = store
            .list_function_declarations()
            .expect("list function declarations");
        assert!(
            declarations.is_empty(),
            "expected no function declarations: {declarations:?}"
        );

        let _ = fs::remove_file(&db_path);
    }

    fn sample_call_edges() -> Vec<CallEdge> {
        vec![
            CallEdge {
                caller_usr: "c:@F@describe#&1$@N@geometry@S@Shape#".to_owned(),
                resolution: CallResolution::Resolved {
                    callee_usr: "c:@N@geometry@S@Shape@F@area#1#".to_owned(),
                    is_dynamic_dispatch: true,
                },
                file: "/workspace/src/shapes.cpp".to_owned(),
                line: 10,
                column: 19,
            },
            CallEdge {
                caller_usr: "c:@F@compute#".to_owned(),
                resolution: CallResolution::Resolved {
                    callee_usr: "c:@F@add#I#I#".to_owned(),
                    is_dynamic_dispatch: false,
                },
                file: "/workspace/src/math.cpp".to_owned(),
                line: 20,
                column: 18,
            },
            CallEdge {
                caller_usr: "c:@F@apply#".to_owned(),
                resolution: CallResolution::Unresolved {
                    reason: "call target is not statically a function".to_owned(),
                },
                file: "/workspace/src/math.cpp".to_owned(),
                line: 25,
                column: 12,
            },
        ]
    }

    #[test]
    fn round_trips_call_edges() {
        let db_path = temp_db_path("project-calls");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_call_edges(&sample_call_edges())
            .expect("persist call edges");

        let callers_of_add = store
            .list_callers_for("c:@F@add#I#I#")
            .expect("list callers of add");
        assert_eq!(
            callers_of_add,
            vec![sample_call_edges()[1].clone()],
            "expected exactly compute's call to add"
        );

        let callers_of_area = store
            .list_callers_for("c:@N@geometry@S@Shape@F@area#1#")
            .expect("list callers of area");
        assert_eq!(callers_of_area, vec![sample_call_edges()[0].clone()]);

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn replacing_call_edges_clears_previous_entries() {
        let db_path = temp_db_path("project-calls-replace");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_call_edges(&sample_call_edges())
            .expect("persist call edges");
        store.replace_call_edges(&[]).expect("clear call edges");

        let callers = store
            .list_callers_for("c:@F@add#I#I#")
            .expect("list callers of add");
        assert!(callers.is_empty(), "expected no callers: {callers:?}");

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn counts_callers_per_function() {
        let db_path = temp_db_path("project-call-counts");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_call_edges(&sample_call_edges())
            .expect("persist call edges");

        let counts = store.call_counts().expect("compute call counts");
        assert_eq!(counts.get("c:@F@add#I#I#"), Some(&1));
        assert_eq!(counts.get("c:@N@geometry@S@Shape@F@area#1#"), Some(&1));
        // The unresolved call in `apply` has no `callee_usr` and so
        // contributes to no function's count.
        assert_eq!(counts.len(), 2);

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn lists_calls_within_a_file() {
        let db_path = temp_db_path("project-calls-in-file");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_call_edges(&sample_call_edges())
            .expect("persist call edges");

        let calls = store
            .list_calls_in_file("/workspace/src/math.cpp")
            .expect("list calls in file");
        assert_eq!(
            calls,
            vec![
                sample_call_edges()[1].clone(),
                sample_call_edges()[2].clone(),
            ],
            "expected both math.cpp calls, ordered by line, and none from shapes.cpp"
        );

        let _ = fs::remove_file(&db_path);
    }

    fn sample_type_mapping() -> MappingDecision {
        MappingDecision {
            type_usr: "c:@S@Ponto".to_owned(),
            option_id: "classe-direta".to_owned(),
            decided_at: "2026-08-12T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn round_trips_type_mappings() {
        let db_path = temp_db_path("project-type-mappings");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .set_type_mapping(&sample_type_mapping())
            .expect("persist type mapping");

        let decisions = store.list_type_mappings().expect("list type mappings");
        assert_eq!(decisions, vec![sample_type_mapping()]);

        let _ = fs::remove_file(&db_path);
    }

    /// US-7 criterion 4: reopening a project preserves the decision — proven
    /// here at the persistence layer by opening the same database twice.
    #[test]
    fn reopening_the_project_preserves_the_recorded_type_mapping() {
        let db_path = temp_db_path("project-type-mappings-reopen");
        let mut store = ProjectStore::open(&db_path).expect("open project store");
        store
            .set_type_mapping(&sample_type_mapping())
            .expect("persist type mapping");
        drop(store);

        let reopened = ProjectStore::open(&db_path).expect("reopen project store");
        let decisions = reopened.list_type_mappings().expect("list type mappings");
        assert_eq!(decisions, vec![sample_type_mapping()]);

        let _ = fs::remove_file(&db_path);
    }

    /// Setting a mapping twice for the same `type_usr` updates it in place
    /// (upsert on the primary key) rather than erroring or duplicating.
    #[test]
    fn setting_a_type_mapping_again_updates_it_in_place() {
        let db_path = temp_db_path("project-type-mappings-update");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .set_type_mapping(&sample_type_mapping())
            .expect("persist type mapping");
        let updated = MappingDecision {
            option_id: "codigo-ponte".to_owned(),
            decided_at: "2026-08-13T00:00:00Z".to_owned(),
            ..sample_type_mapping()
        };
        store
            .set_type_mapping(&updated)
            .expect("update type mapping");

        let decisions = store.list_type_mappings().expect("list type mappings");
        assert_eq!(decisions, vec![updated]);

        let _ = fs::remove_file(&db_path);
    }

    /// Regression test: unlike every other catalog table (`type_declarations`
    /// itself, `type_dependencies`, `source_files`, ...), which is a
    /// wholesale DELETE+replace on every re-extraction since it describes
    /// derived state, `type_mappings` holds a *user decision* — wiping it on
    /// every re-extraction would silently discard a choice the user made.
    /// But a decision recorded for a `type_usr` that no longer exists in the
    /// fresh catalog (the type was renamed or removed, so `libclang`
    /// assigned it a different/no USR) is dead weight pointing at nothing —
    /// `replace_type_declarations` must prune exactly those orphaned rows,
    /// in the same transaction, while leaving a decision for a type that's
    /// still present untouched.
    #[test]
    fn replacing_type_declarations_prunes_mappings_for_types_no_longer_in_the_catalog() {
        let db_path = temp_db_path("project-type-mappings-prune");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        let surviving_type = TypeDeclaration {
            usr: "c:@S@Ponto".to_owned(),
            ..sample_type_declarations()[0].clone()
        };
        store
            .replace_type_declarations(std::slice::from_ref(&surviving_type))
            .expect("seed initial catalog");
        store
            .set_type_mapping(&sample_type_mapping())
            .expect("persist mapping for the surviving type");
        let renamed_type_mapping = MappingDecision {
            type_usr: "c:@S@PontoAntigo".to_owned(),
            ..sample_type_mapping()
        };
        store
            .set_type_mapping(&renamed_type_mapping)
            .expect("persist mapping for a type about to disappear");

        // Re-extraction: `Ponto` survives, but whatever `c:@S@PontoAntigo`
        // used to name is gone from the fresh catalog (renamed/removed).
        store
            .replace_type_declarations(&[surviving_type])
            .expect("replace with a catalog missing the renamed type");

        let decisions = store.list_type_mappings().expect("list type mappings");
        assert_eq!(
            decisions,
            vec![sample_type_mapping()],
            "the orphaned decision should be pruned, the surviving one kept"
        );

        let _ = fs::remove_file(&db_path);
    }

    fn sample_ir_origin() -> ir::Origin {
        ir::Origin {
            file: "/project/input-source/src/aritmetica.cpp".to_owned(),
            line: 2,
            column: 1,
        }
    }

    fn sample_ir_function() -> ir::Function {
        ir::Function {
            name: "soma".to_owned(),
            usr: "c:@F@soma#I#I#".to_owned(),
            params: vec![
                ir::Param {
                    name: "a".to_owned(),
                    ty: ir::Type::Int,
                },
                ir::Param {
                    name: "b".to_owned(),
                    ty: ir::Type::Int,
                },
            ],
            return_type: ir::Type::Int,
            body: vec![ir::Stmt::Return {
                value: Some(ir::Expr::Binary {
                    op: ir::BinaryOp::Add,
                    lhs: Box::new(ir::Expr::Ref {
                        name: "a".to_owned(),
                        ty: ir::Type::Int,
                        origin: sample_ir_origin(),
                    }),
                    rhs: Box::new(ir::Expr::Ref {
                        name: "b".to_owned(),
                        ty: ir::Type::Int,
                        origin: sample_ir_origin(),
                    }),
                    ty: ir::Type::Int,
                    origin: sample_ir_origin(),
                }),
                origin: sample_ir_origin(),
            }],
            origin: sample_ir_origin(),
        }
    }

    fn sample_ir_record() -> ir::Record {
        ir::Record {
            name: "Ponto".to_owned(),
            usr: "c:@S@Ponto".to_owned(),
            fields: vec![
                ir::Field {
                    name: "x".to_owned(),
                    ty: ir::Type::Double,
                },
                ir::Field {
                    name: "y".to_owned(),
                    ty: ir::Type::Double,
                },
            ],
            origin: sample_ir_origin(),
        }
    }

    /// `project_service::transpile_project` reuses this instead of
    /// reparsing every compilation unit with `libclang` on every request
    /// (the same waste `list_types`/`list_functions` already avoid for
    /// their own catalogs) — round-trip through JSON must reproduce the IR
    /// exactly, `Box`-nested expressions included.
    #[test]
    fn round_trips_ir_functions_and_records() {
        let db_path = temp_db_path("project-ir-round-trip");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_ir(&[sample_ir_function()], &[sample_ir_record()])
            .expect("persist ir");

        let (functions, records) = store.list_ir().expect("list ir");
        assert_eq!(functions, vec![sample_ir_function()]);
        assert_eq!(records, vec![sample_ir_record()]);

        let _ = fs::remove_file(&db_path);
    }

    /// Unlike `type_mappings`, the IR *is* derived catalog data — a second
    /// `replace_ir` (as a reingest would trigger) must wholesale-replace the
    /// previous contents, not accumulate alongside them.
    #[test]
    fn replacing_ir_clears_previous_entries() {
        let db_path = temp_db_path("project-ir-replace");
        let mut store = ProjectStore::open(&db_path).expect("open project store");

        store
            .replace_ir(&[sample_ir_function()], &[sample_ir_record()])
            .expect("persist initial ir");
        store
            .replace_ir(&[], &[])
            .expect("replace with an empty ir");

        let (functions, records) = store.list_ir().expect("list ir");
        assert!(functions.is_empty());
        assert!(records.is_empty());

        let _ = fs::remove_file(&db_path);
    }
}
