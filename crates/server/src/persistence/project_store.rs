use std::fs;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::ingest::CompilationUnit;
use crate::source_catalog::{SourceFile, SourceFileKind};
use crate::type_catalog::{TypeDeclaration, TypeDeclarationKind, TypeDependency};

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
                file TEXT NOT NULL,
                line INTEGER NOT NULL,
                column INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS type_dependencies (
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
            );
            CREATE TABLE IF NOT EXISTS source_files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                kind TEXT NOT NULL
            );",
        )?;

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
                "INSERT INTO type_declarations (name, kind, file, line, column)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    declaration.name,
                    declaration.kind.as_str(),
                    declaration.file,
                    declaration.line,
                    declaration.column
                ],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn list_type_declarations(&self) -> Result<Vec<TypeDeclaration>, PersistenceError> {
        let mut statement = self
            .connection
            .prepare("SELECT name, kind, file, line, column FROM type_declarations ORDER BY id")?;

        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, u32>(4)?,
            ))
        })?;

        let mut declarations = Vec::new();
        for row in rows {
            let (name, kind, file, line, column) = row?;
            let Some(kind) = TypeDeclarationKind::parse(&kind) else {
                continue;
            };

            declarations.push(TypeDeclaration {
                name,
                kind,
                file,
                line,
                column,
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
                    caller_name, caller_kind, caller_file, caller_line, caller_column,
                    callee_name, callee_kind, callee_file, callee_line, callee_column
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    dependency.caller.name,
                    dependency.caller.kind.as_str(),
                    dependency.caller.file,
                    dependency.caller.line,
                    dependency.caller.column,
                    dependency.callee.name,
                    dependency.callee.kind.as_str(),
                    dependency.callee.file,
                    dependency.callee.line,
                    dependency.callee.column,
                ],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn list_type_dependencies(&self) -> Result<Vec<TypeDependency>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT caller_name, caller_kind, caller_file, caller_line, caller_column,
                    callee_name, callee_kind, callee_file, callee_line, callee_column
             FROM type_dependencies ORDER BY id",
        )?;

        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, u32>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, u32>(8)?,
                row.get::<_, u32>(9)?,
            ))
        })?;

        let mut dependencies = Vec::new();
        for row in rows {
            let (
                caller_name,
                caller_kind,
                caller_file,
                caller_line,
                caller_column,
                callee_name,
                callee_kind,
                callee_file,
                callee_line,
                callee_column,
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
                    file: caller_file,
                    line: caller_line,
                    column: caller_column,
                },
                callee: TypeDeclaration {
                    name: callee_name,
                    kind: callee_kind,
                    file: callee_file,
                    line: callee_line,
                    column: callee_column,
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
                file: "/workspace/src/types.h".to_owned(),
                line: 3,
                column: 8,
            },
            TypeDeclaration {
                name: "ANSWER".to_owned(),
                kind: TypeDeclarationKind::Macro,
                file: "/workspace/src/types.h".to_owned(),
                line: 1,
                column: 9,
            },
        ]
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
            file: "/workspace/src/types.h".to_owned(),
            line: 3,
            column: 8,
        };
        let rect = TypeDeclaration {
            name: "Rect".to_owned(),
            kind: TypeDeclarationKind::Struct,
            file: "/workspace/src/types.h".to_owned(),
            line: 8,
            column: 8,
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
}
