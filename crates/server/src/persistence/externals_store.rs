//! Text-file-backed store for `externals.rs`'s manual marks, regex rules,
//! and file marks (`docs/plans/lista-de-externos.md`,
//! `docs/prompts/2026-08-19-mudanca-interacao.md` items 1 and 3). Lives at
//! `<project_dir>/externals.txt`, moved out of `project.db` deliberately:
//! a plain, tab-separated, `#`-commentable text file survives a
//! `project.db` wipe/re-ingest and can be read, diffed, and hand-edited
//! outside syntax-bridge — which `project.db`, a SQLite file, cannot.
//!
//! Whole-file rewrite on every write (read, mutate in memory, write back to
//! a temp file, rename over the original) rather than incremental
//! line-editing: far simpler, and the file is small (one line per mark/rule)
//! so the cost is negligible. A line this parser doesn't recognize (wrong
//! field count, an id that doesn't parse) is skipped rather than failing the
//! whole read, mirroring `effective_external_set`'s own "skip what doesn't
//! parse" stance — consistent with the file being something a user might
//! hand-edit imperfectly.
//!
//! No TOML/YAML crate: this workspace vendors its dependencies for the
//! offline Flatpak build (`vendor/`, `.cargo/config.toml`), and adding a new
//! one requires vendoring it, which needs network access this environment
//! doesn't have. The hand-rolled tab-separated format below needs none.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::PersistenceError;
use crate::externals::{ExternalMark, FileMark, NameRegexRule, PathRegexRule};

const HEADER: &str = "\
# syntax-bridge — lista de itens externos (arquivo texto editável)
# Ver docs/plans/lista-de-externos.md. Gerado e lido pelo syntax-bridge; pode
# ser editado por fora também. Linhas em branco ou iniciadas por '#' são
# ignoradas. Campos separados por TAB.
#
# mark\t<include|exclude>\t<usr>\t<decided_at>
# name_regex\t<id>\t<pattern>\t<created_at>
# path_regex\t<id>\t<pattern>\t<created_at>
# file\t<path>\t<decided_at>
";

#[derive(Debug, Default, Clone, PartialEq)]
struct ExternalsData {
    marks: Vec<ExternalMark>,
    name_regexes: Vec<NameRegexRule>,
    path_regexes: Vec<PathRegexRule>,
    file_marks: Vec<FileMark>,
}

pub struct ExternalsStore {
    path: PathBuf,
}

impl ExternalsStore {
    /// Opens the store for `project_dir` — doesn't touch the filesystem
    /// until the first read or write.
    pub fn open(project_dir: &Path) -> Self {
        Self {
            path: project_dir.join("externals.txt"),
        }
    }

    fn read(&self) -> Result<ExternalsData, PersistenceError> {
        match fs::read_to_string(&self.path) {
            Ok(contents) => Ok(parse(&contents)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ExternalsData::default()),
            Err(error) => Err(error.into()),
        }
    }

    fn write(&self, data: &ExternalsData) -> Result<(), PersistenceError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp_path = self.path.with_extension("txt.tmp");
        fs::write(&tmp_path, serialize(data))?;
        fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }

    pub fn list_marks(&self) -> Result<Vec<ExternalMark>, PersistenceError> {
        Ok(self.read()?.marks)
    }

    /// Upsert by usr — same shape as the old `ProjectStore::set_external_mark`.
    pub fn set_mark(&self, mark: &ExternalMark) -> Result<(), PersistenceError> {
        let mut data = self.read()?;
        data.marks.retain(|existing| existing.usr != mark.usr);
        data.marks.push(mark.clone());
        data.marks.sort_by(|a, b| a.usr.cmp(&b.usr));
        self.write(&data)
    }

    /// Drops marks whose usr isn't in `valid_usrs` — the text-file
    /// equivalent of the old `project_store::prune_external_marks_tx`,
    /// called after ingestion/analysis replaces
    /// `type_declarations`/`function_declarations` so a mark for a usr that
    /// no longer exists in the project doesn't linger forever.
    pub fn prune_marks(&self, valid_usrs: &HashSet<&str>) -> Result<(), PersistenceError> {
        let mut data = self.read()?;
        let before = data.marks.len();
        data.marks
            .retain(|mark| valid_usrs.contains(mark.usr.as_str()));
        if data.marks.len() != before {
            self.write(&data)?;
        }
        Ok(())
    }

    pub fn list_name_regexes(&self) -> Result<Vec<NameRegexRule>, PersistenceError> {
        Ok(self.read()?.name_regexes)
    }

    pub fn add_name_regex(&self, pattern: &str, created_at: &str) -> Result<i64, PersistenceError> {
        let mut data = self.read()?;
        let id = next_id(data.name_regexes.iter().map(|rule| rule.id));
        data.name_regexes.push(NameRegexRule {
            id,
            pattern: pattern.to_owned(),
            created_at: created_at.to_owned(),
        });
        self.write(&data)?;
        Ok(id)
    }

    pub fn remove_name_regex(&self, id: i64) -> Result<(), PersistenceError> {
        let mut data = self.read()?;
        data.name_regexes.retain(|rule| rule.id != id);
        self.write(&data)
    }

    pub fn list_path_regexes(&self) -> Result<Vec<PathRegexRule>, PersistenceError> {
        Ok(self.read()?.path_regexes)
    }

    pub fn add_path_regex(&self, pattern: &str, created_at: &str) -> Result<i64, PersistenceError> {
        let mut data = self.read()?;
        let id = next_id(data.path_regexes.iter().map(|rule| rule.id));
        data.path_regexes.push(PathRegexRule {
            id,
            pattern: pattern.to_owned(),
            created_at: created_at.to_owned(),
        });
        self.write(&data)?;
        Ok(id)
    }

    pub fn remove_path_regex(&self, id: i64) -> Result<(), PersistenceError> {
        let mut data = self.read()?;
        data.path_regexes.retain(|rule| rule.id != id);
        self.write(&data)
    }

    pub fn list_file_marks(&self) -> Result<Vec<FileMark>, PersistenceError> {
        Ok(self.read()?.file_marks)
    }

    /// Sets or clears the persistent mark for `file` — item 3's reversal of
    /// decision 3 (`docs/plans/lista-de-externos.md`): unlike
    /// [`Self::set_mark`], there's no `external: false` entry, since
    /// presence in the list *is* the mark; `external: false` here just
    /// means "remove the entry if it exists".
    pub fn set_file_mark(
        &self,
        file: &str,
        external: bool,
        decided_at: &str,
    ) -> Result<(), PersistenceError> {
        let mut data = self.read()?;
        data.file_marks.retain(|mark| mark.file != file);
        if external {
            data.file_marks.push(FileMark {
                file: file.to_owned(),
                decided_at: decided_at.to_owned(),
            });
            data.file_marks.sort_by(|a, b| a.file.cmp(&b.file));
        }
        self.write(&data)
    }

    /// Drops file marks whose file isn't in `valid_files` — same purpose as
    /// [`Self::prune_marks`], for a file removed from the project (renamed,
    /// deleted, or excluded from ingestion) rather than a usr.
    pub fn prune_file_marks(&self, valid_files: &HashSet<&str>) -> Result<(), PersistenceError> {
        let mut data = self.read()?;
        let before = data.file_marks.len();
        data.file_marks
            .retain(|mark| valid_files.contains(mark.file.as_str()));
        if data.file_marks.len() != before {
            self.write(&data)?;
        }
        Ok(())
    }

    /// One-time migration off the old SQLite-backed store
    /// (`project_store::ProjectStore`'s retired `external_marks`/
    /// `external_name_regexes`/`external_path_regexes` tables). Writes the
    /// harvested rows here only if this file doesn't exist yet, so it never
    /// clobbers a file a user has already started hand-editing, and only if
    /// there's anything to migrate, so a brand-new project never gets an
    /// empty `externals.txt` it didn't ask for.
    pub fn migrate_from_legacy(
        &self,
        marks: Vec<ExternalMark>,
        name_regexes: Vec<NameRegexRule>,
        path_regexes: Vec<PathRegexRule>,
    ) -> Result<(), PersistenceError> {
        if self.path.exists() {
            return Ok(());
        }
        if marks.is_empty() && name_regexes.is_empty() && path_regexes.is_empty() {
            return Ok(());
        }
        self.write(&ExternalsData {
            marks,
            name_regexes,
            path_regexes,
            file_marks: Vec::new(),
        })
    }
}

fn next_id(existing: impl Iterator<Item = i64>) -> i64 {
    existing.max().unwrap_or(0) + 1
}

fn parse(contents: &str) -> ExternalsData {
    let mut data = ExternalsData::default();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["mark", "include", usr, decided_at] => data.marks.push(ExternalMark {
                usr: (*usr).to_owned(),
                external: true,
                decided_at: (*decided_at).to_owned(),
            }),
            ["mark", "exclude", usr, decided_at] => data.marks.push(ExternalMark {
                usr: (*usr).to_owned(),
                external: false,
                decided_at: (*decided_at).to_owned(),
            }),
            ["name_regex", id, pattern, created_at] => {
                if let Ok(id) = id.parse::<i64>() {
                    data.name_regexes.push(NameRegexRule {
                        id,
                        pattern: (*pattern).to_owned(),
                        created_at: (*created_at).to_owned(),
                    });
                }
            }
            ["path_regex", id, pattern, created_at] => {
                if let Ok(id) = id.parse::<i64>() {
                    data.path_regexes.push(PathRegexRule {
                        id,
                        pattern: (*pattern).to_owned(),
                        created_at: (*created_at).to_owned(),
                    });
                }
            }
            ["file", file, decided_at] => data.file_marks.push(FileMark {
                file: (*file).to_owned(),
                decided_at: (*decided_at).to_owned(),
            }),
            _ => {} // unrecognized line: skip rather than fail the whole read
        }
    }
    data
}

fn serialize(data: &ExternalsData) -> String {
    let mut out = String::from(HEADER);
    for mark in &data.marks {
        let kind = if mark.external { "include" } else { "exclude" };
        out.push_str(&format!(
            "mark\t{kind}\t{}\t{}\n",
            mark.usr, mark.decided_at
        ));
    }
    for rule in &data.name_regexes {
        out.push_str(&format!(
            "name_regex\t{}\t{}\t{}\n",
            rule.id, rule.pattern, rule.created_at
        ));
    }
    for rule in &data.path_regexes {
        out.push_str(&format!(
            "path_regex\t{}\t{}\t{}\n",
            rule.id, rule.pattern, rule.created_at
        ));
    }
    for mark in &data.file_marks {
        out.push_str(&format!("file\t{}\t{}\n", mark.file, mark.decided_at));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project_dir(label: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "syntax-bridge-externals-store-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        path
    }

    #[test]
    fn a_fresh_project_has_no_marks_regexes_or_file_marks() {
        let store = ExternalsStore::open(&temp_project_dir("fresh"));
        assert!(store.list_marks().unwrap().is_empty());
        assert!(store.list_name_regexes().unwrap().is_empty());
        assert!(store.list_path_regexes().unwrap().is_empty());
        assert!(store.list_file_marks().unwrap().is_empty());
    }

    #[test]
    fn round_trips_a_manual_mark() {
        let store = ExternalsStore::open(&temp_project_dir("mark-round-trip"));
        store
            .set_mark(&ExternalMark {
                usr: "c:@F@f#".to_owned(),
                external: true,
                decided_at: "2026-08-19T00:00:00Z".to_owned(),
            })
            .unwrap();

        let marks = store.list_marks().unwrap();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].usr, "c:@F@f#");
        assert!(marks[0].external);
    }

    #[test]
    fn setting_a_mark_again_updates_it_in_place() {
        let store = ExternalsStore::open(&temp_project_dir("mark-update"));
        store
            .set_mark(&ExternalMark {
                usr: "c:@F@f#".to_owned(),
                external: true,
                decided_at: "2026-08-19T00:00:00Z".to_owned(),
            })
            .unwrap();
        store
            .set_mark(&ExternalMark {
                usr: "c:@F@f#".to_owned(),
                external: false,
                decided_at: "2026-08-19T00:00:01Z".to_owned(),
            })
            .unwrap();

        let marks = store.list_marks().unwrap();
        assert_eq!(marks.len(), 1);
        assert!(!marks[0].external);
    }

    #[test]
    fn prune_marks_drops_usrs_not_in_the_valid_set() {
        let store = ExternalsStore::open(&temp_project_dir("prune-marks"));
        store
            .set_mark(&ExternalMark {
                usr: "c:@F@stale#".to_owned(),
                external: true,
                decided_at: "2026-08-19T00:00:00Z".to_owned(),
            })
            .unwrap();
        store
            .set_mark(&ExternalMark {
                usr: "c:@F@fresh#".to_owned(),
                external: true,
                decided_at: "2026-08-19T00:00:00Z".to_owned(),
            })
            .unwrap();

        let valid: HashSet<&str> = ["c:@F@fresh#"].into_iter().collect();
        store.prune_marks(&valid).unwrap();

        let marks = store.list_marks().unwrap();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].usr, "c:@F@fresh#");
    }

    #[test]
    fn round_trips_name_and_path_regexes_with_increasing_ids() {
        let store = ExternalsStore::open(&temp_project_dir("regex-round-trip"));
        let name_id = store
            .add_name_regex("^humlib::", "2026-08-19T00:00:00Z")
            .unwrap();
        let second_name_id = store
            .add_name_regex("^Foo::", "2026-08-19T00:00:01Z")
            .unwrap();
        let path_id = store
            .add_path_regex("^third_party/", "2026-08-19T00:00:02Z")
            .unwrap();

        assert_eq!(name_id, 1);
        assert_eq!(second_name_id, 2);
        assert_eq!(path_id, 1);

        let name_rules = store.list_name_regexes().unwrap();
        assert_eq!(name_rules.len(), 2);
        assert_eq!(name_rules[0].pattern, "^humlib::");
        assert_eq!(name_rules[1].pattern, "^Foo::");

        let path_rules = store.list_path_regexes().unwrap();
        assert_eq!(path_rules.len(), 1);
        assert_eq!(path_rules[0].pattern, "^third_party/");

        store.remove_name_regex(name_id).unwrap();
        store.remove_path_regex(path_id).unwrap();

        assert_eq!(store.list_name_regexes().unwrap().len(), 1);
        assert!(store.list_path_regexes().unwrap().is_empty());
    }

    #[test]
    fn round_trips_a_file_mark() {
        let store = ExternalsStore::open(&temp_project_dir("file-mark-round-trip"));
        store
            .set_file_mark(
                "/project/input-source/third_party/humlib/humlib.cpp",
                true,
                "2026-08-19T00:00:00Z",
            )
            .unwrap();

        let marks = store.list_file_marks().unwrap();
        assert_eq!(marks.len(), 1);
        assert_eq!(
            marks[0].file,
            "/project/input-source/third_party/humlib/humlib.cpp"
        );
    }

    #[test]
    fn setting_a_file_mark_to_external_false_removes_it() {
        let store = ExternalsStore::open(&temp_project_dir("file-mark-remove"));
        store
            .set_file_mark(
                "/project/third_party/humlib.cpp",
                true,
                "2026-08-19T00:00:00Z",
            )
            .unwrap();
        store
            .set_file_mark(
                "/project/third_party/humlib.cpp",
                false,
                "2026-08-19T00:00:01Z",
            )
            .unwrap();

        assert!(store.list_file_marks().unwrap().is_empty());
    }

    #[test]
    fn prune_file_marks_drops_files_not_in_the_valid_set() {
        let store = ExternalsStore::open(&temp_project_dir("prune-file-marks"));
        store
            .set_file_mark("/project/stale.cpp", true, "2026-08-19T00:00:00Z")
            .unwrap();
        store
            .set_file_mark("/project/fresh.cpp", true, "2026-08-19T00:00:00Z")
            .unwrap();

        let valid: HashSet<&str> = ["/project/fresh.cpp"].into_iter().collect();
        store.prune_file_marks(&valid).unwrap();

        let marks = store.list_file_marks().unwrap();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].file, "/project/fresh.cpp");
    }

    #[test]
    fn a_line_with_the_wrong_field_count_is_skipped_instead_of_failing_the_read() {
        let dir = temp_project_dir("malformed-line");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("externals.txt"),
            "mark\tinclude\tc:@F@only_two_fields#\n\
             mark\tinclude\tc:@F@good#\t2026-08-19T00:00:00Z\n",
        )
        .unwrap();

        let store = ExternalsStore::open(&dir);
        let marks = store.list_marks().unwrap();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].usr, "c:@F@good#");
    }

    #[test]
    fn migrate_from_legacy_writes_the_file_only_once() {
        let dir = temp_project_dir("migrate-legacy");
        let store = ExternalsStore::open(&dir);
        store
            .migrate_from_legacy(
                vec![ExternalMark {
                    usr: "c:@F@f#".to_owned(),
                    external: true,
                    decided_at: "2026-08-19T00:00:00Z".to_owned(),
                }],
                vec![NameRegexRule {
                    id: 1,
                    pattern: "^humlib::".to_owned(),
                    created_at: "2026-08-19T00:00:00Z".to_owned(),
                }],
                Vec::new(),
            )
            .unwrap();
        assert_eq!(store.list_marks().unwrap().len(), 1);
        assert_eq!(store.list_name_regexes().unwrap().len(), 1);

        // A second migration attempt (e.g. a stale `user_version` re-check)
        // must not clobber marks the user may have added since.
        store
            .set_mark(&ExternalMark {
                usr: "c:@F@g#".to_owned(),
                external: true,
                decided_at: "2026-08-19T00:00:01Z".to_owned(),
            })
            .unwrap();
        store
            .migrate_from_legacy(Vec::new(), Vec::new(), Vec::new())
            .unwrap();

        assert_eq!(store.list_marks().unwrap().len(), 2);
    }

    #[test]
    fn migrate_from_legacy_is_a_noop_when_there_is_nothing_to_migrate() {
        let dir = temp_project_dir("migrate-legacy-empty");
        let store = ExternalsStore::open(&dir);
        store
            .migrate_from_legacy(Vec::new(), Vec::new(), Vec::new())
            .unwrap();

        assert!(!dir.join("externals.txt").exists());
    }
}
