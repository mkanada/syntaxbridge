use std::collections::HashMap;
use std::fs;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::ingest::CompilationUnit;
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
            CREATE INDEX IF NOT EXISTS idx_type_usages_type_usr ON type_usages(type_usr);",
        )?;

        migrate_type_columns(&connection)?;

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
}
