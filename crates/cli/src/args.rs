//! Hand-rolled argument parsing — no `clap`, for the same reason `http.rs`
//! hand-rolls its client: the command surface is small and fixed
//! (`docs/plans/interface-de-linha-de-comando.md`), so parsing it by hand
//! keeps this crate free of a dependency that would need vendoring.

use std::collections::HashMap;
use std::path::PathBuf;

pub const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:37651";
pub const DEFAULT_CALLERS_DEPTH: u32 = 2;

#[derive(Debug, PartialEq)]
pub enum OutputFormat {
    Tree,
    Dot,
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Help,
    Version,
    Init {
        archive: PathBuf,
        name: Option<String>,
        workspace: Option<PathBuf>,
    },
    Open {
        path: PathBuf,
    },
    ProjectsList,
    ProjectsForget {
        path: PathBuf,
    },
    Files,
    Cat {
        file: String,
    },
    Types {
        kind: Option<String>,
        namespace: Option<String>,
    },
    TypesUsages {
        needle: String,
    },
    Pointers,
    Functions {
        filter: Option<String>,
    },
    FunctionsCallers {
        needle: String,
        depth: u32,
        format: OutputFormat,
    },
    FunctionsCallsInFile {
        file: String,
    },
    Transpile,
}

#[derive(Debug, PartialEq)]
pub struct Cli {
    pub server_url: String,
    pub project: Option<PathBuf>,
    pub json: bool,
    pub command: Command,
}

pub fn parse(args: &[String]) -> Result<Cli, String> {
    let mut server_url = DEFAULT_SERVER_URL.to_owned();
    let mut project: Option<PathBuf> = None;
    let mut json = false;
    let mut rest: Vec<String> = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--server-url" => {
                server_url = iter
                    .next()
                    .ok_or("--server-url requer um valor")?
                    .to_owned()
            }
            "--project" => {
                project = Some(PathBuf::from(
                    iter.next().ok_or("--project requer um valor")?,
                ))
            }
            "--json" => json = true,
            "--help" | "-h" => {
                return Ok(Cli {
                    server_url,
                    project,
                    json,
                    command: Command::Help,
                });
            }
            "--version" => {
                return Ok(Cli {
                    server_url,
                    project,
                    json,
                    command: Command::Version,
                });
            }
            other => rest.push(other.to_owned()),
        }
    }

    let command = parse_command(&rest)?;
    Ok(Cli {
        server_url,
        project,
        json,
        command,
    })
}

struct Cursor<'a> {
    items: &'a [String],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(items: &'a [String]) -> Self {
        Self { items, pos: 0 }
    }

    fn peek(&self) -> Option<&'a str> {
        self.items.get(self.pos).map(String::as_str)
    }

    fn next(&mut self) -> Option<&'a str> {
        let item = self.peek();
        if item.is_some() {
            self.pos += 1;
        }
        item
    }

    fn expect(&mut self, what: &str) -> Result<&'a str, String> {
        self.next()
            .ok_or_else(|| format!("esperado {what}, nada foi informado"))
    }

    fn expect_end(&self) -> Result<(), String> {
        match self.peek() {
            None => Ok(()),
            Some(extra) => Err(format!("argumento inesperado: {extra}")),
        }
    }
}

/// Parses the remaining tokens as `--flag value` pairs — every
/// command-specific flag in this CLI takes a value, so this one loop
/// covers `init`, `types`, `functions` and `functions callers`.
fn parse_value_flags(
    cursor: &mut Cursor,
    allowed: &[&str],
) -> Result<HashMap<String, String>, String> {
    let mut flags = HashMap::new();
    while let Some(token) = cursor.peek() {
        if !allowed.contains(&token) {
            return Err(format!("opção desconhecida: {token}"));
        }
        let flag = cursor.next().unwrap().to_owned();
        let value = cursor.expect(&format!("um valor para {flag}"))?.to_owned();
        flags.insert(flag, value);
    }
    Ok(flags)
}

fn parse_command(rest: &[String]) -> Result<Command, String> {
    let mut cursor = Cursor::new(rest);
    let name = cursor.expect("um comando (ex.: init, open, types, functions)")?;

    match name {
        "init" => {
            let archive =
                PathBuf::from(cursor.expect("o caminho de um arquivo/diretório de origem")?);
            let flags = parse_value_flags(&mut cursor, &["--name", "--workspace"])?;
            Ok(Command::Init {
                archive,
                name: flags.get("--name").cloned(),
                workspace: flags.get("--workspace").cloned().map(PathBuf::from),
            })
        }
        "open" => {
            let path = PathBuf::from(cursor.expect("o caminho de um projeto")?);
            cursor.expect_end()?;
            Ok(Command::Open { path })
        }
        "projects" => match cursor.peek() {
            Some("forget") => {
                cursor.next();
                let path = PathBuf::from(cursor.expect("o caminho de um projeto")?);
                cursor.expect_end()?;
                Ok(Command::ProjectsForget { path })
            }
            None => Ok(Command::ProjectsList),
            Some(other) => Err(format!("subcomando desconhecido para projects: {other}")),
        },
        "files" => {
            cursor.expect_end()?;
            Ok(Command::Files)
        }
        "cat" => {
            let file = cursor
                .expect("o caminho de um arquivo dentro do projeto")?
                .to_owned();
            cursor.expect_end()?;
            Ok(Command::Cat { file })
        }
        "types" => match cursor.peek() {
            Some("usages") => {
                cursor.next();
                let needle = cursor.expect("o nome ou usr de um tipo")?.to_owned();
                cursor.expect_end()?;
                Ok(Command::TypesUsages { needle })
            }
            _ => {
                let flags = parse_value_flags(&mut cursor, &["--kind", "--namespace"])?;
                Ok(Command::Types {
                    kind: flags.get("--kind").cloned(),
                    namespace: flags.get("--namespace").cloned(),
                })
            }
        },
        "pointers" => {
            cursor.expect_end()?;
            Ok(Command::Pointers)
        }
        "functions" => match cursor.peek() {
            Some("callers") => {
                cursor.next();
                let needle = cursor.expect("o nome ou usr de uma função")?.to_owned();
                let flags = parse_value_flags(&mut cursor, &["--depth", "--format"])?;
                let depth = match flags.get("--depth") {
                    Some(raw) => raw
                        .parse::<u32>()
                        .map_err(|_| format!("--depth inválido: {raw}"))?,
                    None => DEFAULT_CALLERS_DEPTH,
                };
                let format = match flags.get("--format").map(String::as_str) {
                    None | Some("tree") => OutputFormat::Tree,
                    Some("dot") => OutputFormat::Dot,
                    Some(other) => {
                        return Err(format!("--format inválido: {other} (use tree ou dot)"));
                    }
                };
                Ok(Command::FunctionsCallers {
                    needle,
                    depth,
                    format,
                })
            }
            Some("calls-in-file") => {
                cursor.next();
                let flags = parse_value_flags(&mut cursor, &["--file"])?;
                let file = flags
                    .get("--file")
                    .cloned()
                    .ok_or("functions calls-in-file requer --file <path>")?;
                Ok(Command::FunctionsCallsInFile { file })
            }
            _ => {
                let flags = parse_value_flags(&mut cursor, &["--filter"])?;
                Ok(Command::Functions {
                    filter: flags.get("--filter").cloned(),
                })
            }
        },
        "transpile" => {
            cursor.expect_end()?;
            Ok(Command::Transpile)
        }
        other => Err(format!("comando desconhecido: {other} (use --help)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_global_flags_from_anywhere_in_the_arguments() {
        let cli = parse(&args(&[
            "--json",
            "types",
            "--server-url",
            "http://example:1234",
            "--kind",
            "struct",
        ]))
        .expect("parse");

        assert_eq!(cli.server_url, "http://example:1234");
        assert!(cli.json);
        assert_eq!(
            cli.command,
            Command::Types {
                kind: Some("struct".to_owned()),
                namespace: None
            }
        );
    }

    #[test]
    fn defaults_server_url_and_json_when_not_given() {
        let cli = parse(&args(&["pointers"])).expect("parse");
        assert_eq!(cli.server_url, DEFAULT_SERVER_URL);
        assert!(!cli.json);
        assert!(cli.project.is_none());
    }

    #[test]
    fn parses_project_override() {
        let cli = parse(&args(&["--project", "/tmp/proj", "files"])).expect("parse");
        assert_eq!(cli.project, Some(PathBuf::from("/tmp/proj")));
    }

    #[test]
    fn parses_init_with_optional_flags() {
        let cli = parse(&args(&[
            "init",
            "/tmp/archive.tar.gz",
            "--name",
            "counter",
            "--workspace",
            "/tmp/ws",
        ]))
        .expect("parse");

        assert_eq!(
            cli.command,
            Command::Init {
                archive: PathBuf::from("/tmp/archive.tar.gz"),
                name: Some("counter".to_owned()),
                workspace: Some(PathBuf::from("/tmp/ws")),
            }
        );
    }

    #[test]
    fn parses_init_without_optional_flags() {
        let cli = parse(&args(&["init", "/tmp/archive.tar.gz"])).expect("parse");
        assert_eq!(
            cli.command,
            Command::Init {
                archive: PathBuf::from("/tmp/archive.tar.gz"),
                name: None,
                workspace: None,
            }
        );
    }

    #[test]
    fn parses_projects_list_and_forget() {
        assert_eq!(
            parse(&args(&["projects"])).unwrap().command,
            Command::ProjectsList
        );
        assert_eq!(
            parse(&args(&["projects", "forget", "/tmp/x"]))
                .unwrap()
                .command,
            Command::ProjectsForget {
                path: PathBuf::from("/tmp/x")
            }
        );
    }

    #[test]
    fn parses_types_usages() {
        assert_eq!(
            parse(&args(&["types", "usages", "Triangulo"]))
                .unwrap()
                .command,
            Command::TypesUsages {
                needle: "Triangulo".to_owned()
            }
        );
    }

    #[test]
    fn parses_functions_callers_with_defaults() {
        let cli = parse(&args(&["functions", "callers", "Triangulo::area"])).unwrap();
        assert_eq!(
            cli.command,
            Command::FunctionsCallers {
                needle: "Triangulo::area".to_owned(),
                depth: DEFAULT_CALLERS_DEPTH,
                format: OutputFormat::Tree,
            }
        );
    }

    #[test]
    fn parses_functions_callers_with_depth_and_dot_format() {
        let cli = parse(&args(&[
            "functions",
            "callers",
            "Triangulo::area",
            "--depth",
            "5",
            "--format",
            "dot",
        ]))
        .unwrap();
        assert_eq!(
            cli.command,
            Command::FunctionsCallers {
                needle: "Triangulo::area".to_owned(),
                depth: 5,
                format: OutputFormat::Dot,
            }
        );
    }

    #[test]
    fn parses_functions_calls_in_file() {
        let cli = parse(&args(&["functions", "calls-in-file", "--file", "main.cpp"])).unwrap();
        assert_eq!(
            cli.command,
            Command::FunctionsCallsInFile {
                file: "main.cpp".to_owned()
            }
        );
    }

    #[test]
    fn rejects_an_unknown_command() {
        let error = parse(&args(&["frobnicate"])).unwrap_err();
        assert!(error.contains("frobnicate"));
    }

    #[test]
    fn rejects_an_unknown_flag() {
        let error = parse(&args(&["types", "--bogus", "x"])).unwrap_err();
        assert!(error.contains("--bogus"));
    }

    #[test]
    fn rejects_a_missing_flag_value() {
        let error = parse(&args(&["types", "--kind"])).unwrap_err();
        assert!(error.contains("--kind"));
    }

    #[test]
    fn help_and_version_short_circuit_command_parsing() {
        assert_eq!(parse(&args(&["--help"])).unwrap().command, Command::Help);
        assert_eq!(
            parse(&args(&["types", "--version"])).unwrap().command,
            Command::Version
        );
    }
}
