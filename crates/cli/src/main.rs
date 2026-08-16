use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use syntax_bridge_cli::args::{self, Command, OutputFormat};
use syntax_bridge_cli::commands::{self, CommandError};
use syntax_bridge_cli::http::Client;
use syntax_bridge_cli::project;

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    let cli = match args::parse(&argv) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("erro: {message}\n");
            eprint!("{}", usage());
            return ExitCode::FAILURE;
        }
    };

    match cli.command {
        Command::Help => {
            print!("{}", usage());
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("syntax-bridge {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        command => {
            let client = Client::new(cli.server_url);
            match run(&client, cli.project, cli.json, command) {
                Ok(output) => {
                    print!("{output}");
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    eprintln!("erro: {message}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn run(
    client: &Client,
    project_override: Option<PathBuf>,
    json: bool,
    command: Command,
) -> Result<String, String> {
    match command {
        Command::Help | Command::Version => unreachable!("handled in main"),

        Command::Init {
            archive,
            name,
            workspace,
        } => run_init(client, archive, name, workspace, json),

        Command::Open { path } => commands::projects::request_open(client, &path)
            .map(|body| commands::projects::render_open(json, &body))
            .map_err(|e| e.to_string()),

        Command::ProjectsList => commands::projects::request_list(client)
            .map(|body| commands::projects::render_list(json, &body))
            .map_err(|e| e.to_string()),

        Command::ProjectsForget { path } => commands::projects::request_forget(client, &path)
            .map(|body| commands::projects::render_forget(json, &body, &path))
            .map_err(|e| e.to_string()),

        Command::Status { job_id } => commands::status::request_status(client, &job_id)
            .map(|body| commands::status::render_status(json, &body))
            .map_err(|e| e.to_string()),

        Command::Files => with_project(project_override, |project_dir| {
            commands::files::request_files(client, project_dir)
                .map(|body| commands::files::render_files(json, &body))
        }),

        Command::Cat { file } => with_project(project_override, |project_dir| {
            commands::files::request_cat(client, project_dir, &file)
                .map(|body| commands::files::render_cat(json, &body))
        }),

        Command::Types { kind, namespace } => with_project(project_override, |project_dir| {
            commands::types::request_types(client, project_dir).map(|body| {
                commands::types::render_types(
                    json,
                    &body,
                    &commands::types::TypeFilters {
                        kind: kind.as_deref(),
                        namespace: namespace.as_deref(),
                    },
                )
            })
        }),

        Command::TypesUsages { needle } => with_project(project_override, |project_dir| {
            commands::types::request_usages(client, project_dir, &needle)
                .map(|body| commands::types::render_usages(json, &body))
        }),

        Command::Pointers => with_project(project_override, |project_dir| {
            commands::pointers::request_pointers(client, project_dir)
                .map(|body| commands::pointers::render_pointers(json, &body))
        }),

        Command::Functions { filter } => with_project(project_override, |project_dir| {
            let class_names = fetch_class_names(client, project_dir)?;
            commands::functions::request_functions(client, project_dir).map(|body| {
                commands::functions::render_functions(json, &body, filter.as_deref(), &class_names)
            })
        }),

        Command::FunctionsCallers {
            needle,
            depth,
            format,
        } => with_project(project_override, |project_dir| {
            run_callers(client, project_dir, &needle, depth, &format, json)
        }),

        Command::FunctionsCallsInFile { file } => with_project(project_override, |project_dir| {
            commands::functions::request_calls_in_file(client, project_dir, &file)
                .map(|body| commands::functions::render_calls_in_file(json, &body))
        }),

        Command::Transpile => with_project(project_override, |project_dir| {
            commands::transpile::request_transpile(client, project_dir)
                .map(|body| commands::transpile::render_outcome(json, &body))
        }),
    }
}

fn with_project(
    project_override: Option<PathBuf>,
    f: impl FnOnce(&Path) -> Result<String, CommandError>,
) -> Result<String, String> {
    let project_dir = resolve_project_dir(project_override)?;
    f(&project_dir).map_err(|error| error.to_string())
}

fn resolve_project_dir(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let cwd = env::current_dir().map_err(|error| error.to_string())?;
    project::find_project_dir(&cwd).ok_or_else(|| {
        "nenhum projeto encontrado a partir daqui; rode dentro de um projeto, use --project <path>, \
         ou crie um com `syntax-bridge init`"
            .to_owned()
    })
}

/// Fetches the type catalog once and reduces it to `usr -> name`, the map
/// `commands::functions::qualified_name` needs to tell methods on
/// different classes apart (see that module's doc comment on
/// `ClassNames`).
fn fetch_class_names(
    client: &Client,
    project_dir: &Path,
) -> Result<commands::functions::ClassNames, CommandError> {
    let types_body = commands::types::request_types(client, project_dir)?;
    let empty = Vec::new();
    let types = types_body["types"].as_array().unwrap_or(&empty);
    Ok(commands::functions::class_names_from_types(types))
}

fn run_callers(
    client: &Client,
    project_dir: &Path,
    needle: &str,
    depth: u32,
    format: &OutputFormat,
    json: bool,
) -> Result<String, CommandError> {
    let class_names = fetch_class_names(client, project_dir)?;
    let functions_body = commands::functions::request_functions(client, project_dir)?;
    let empty = Vec::new();
    let functions = functions_body["functions"].as_array().unwrap_or(&empty);
    let usr = commands::functions::resolve_function(functions, needle, &class_names)?;
    let tree = commands::functions::build_callers_tree(
        client,
        project_dir,
        functions,
        &class_names,
        &usr,
        depth,
    )?;

    Ok(if json {
        commands::functions::render_callers_tree_json(&tree)
    } else {
        match format {
            OutputFormat::Tree => commands::functions::render_callers_tree_text(&tree),
            OutputFormat::Dot => commands::functions::render_callers_tree_dot(&tree),
        }
    })
}

fn run_init(
    client: &Client,
    archive: PathBuf,
    name: Option<String>,
    workspace: Option<PathBuf>,
    json: bool,
) -> Result<String, String> {
    let name = name.unwrap_or_else(|| default_project_name(&archive));
    let workspace_dir = match workspace {
        Some(dir) => dir,
        None => env::current_dir().map_err(|error| error.to_string())?,
    };

    let outcome = commands::init::run(client, &name, &workspace_dir, &archive, |line| {
        if !json {
            print!("{line}");
            let _ = std::io::stdout().flush();
        }
    })
    .map_err(|error| error.to_string())?;

    Ok(commands::init::render_outcome(json, &outcome))
}

/// Derives a default project name from the archive's file name — strips
/// `.tar.gz`/`.tgz`/`.zip`-style extensions so `verovio-6.2.0.tar.gz`
/// suggests `verovio-6.2.0`, not `verovio-6.2.0.tar`.
fn default_project_name(archive: &Path) -> String {
    let file_name = archive
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_owned());

    let without_known_extension = file_name
        .strip_suffix(".tar.gz")
        .or_else(|| file_name.strip_suffix(".tgz"))
        .or_else(|| file_name.strip_suffix(".tar"))
        .or_else(|| file_name.strip_suffix(".zip"))
        .unwrap_or(&file_name);

    without_known_extension.to_owned()
}

fn usage() -> String {
    "\
Syntax Bridge — CLI para o transpilador C/C++ → Dart.
Consulte docs/plans/interface-de-linha-de-comando.md para o desenho completo.

USO:
  syntax-bridge [opções globais] <comando> [args]

OPÇÕES GLOBAIS:
  --server-url <url>   Endereço do servidor (padrão: http://127.0.0.1:37651)
  --project <path>     Usa este projeto em vez de resolver pelo diretório atual
  --json                Imprime a resposta da API como JSON, sem formatação
  --help, -h            Mostra esta mensagem
  --version              Mostra a versão

FORA DE UM PROJETO:
  init <archive> [--name NOME] [--workspace DIR]   Cria um projeto
  open <path>                                       Abre/registra um projeto existente
  projects                                           Lista projetos recentes
  projects forget <path>                             Remove um projeto da lista de recentes
  status <job-id>                                     Consulta o progresso de um job (ex.: de outro terminal enquanto `init` roda)

DENTRO DE UM PROJETO (resolvido a partir do diretório atual, como o git faz com .git):
  files                                              Lista os arquivos-fonte
  cat <file>                                         Mostra o conteúdo de um arquivo
  types [--kind K] [--namespace N]                   Lista o catálogo de tipos
  types usages <type>                                Lista os usos de um tipo
  pointers                                            Lista o catálogo de ponteiros
  functions [--filter STR]                            Lista funções/métodos/macros
  functions callers <name> [--depth N] [--format tree|dot]   Grafo de quem chama <name>
  functions calls-in-file --file <path>               Chamadas feitas dentro de um arquivo
  transpile                                           Gera o pacote Dart
"
    .to_owned()
}
