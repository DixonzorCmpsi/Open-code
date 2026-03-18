use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc;

use clap::{Parser, Subcommand, ValueEnum};
use clawc::ast::Document;
use clawc::codegen;
use clawc::config::{BuildLanguage, OpenClawConfig};
use clawc::errors::CompilerError;
use clawc::{parser, semantic};
use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(author, version, about = "OpenClaw workspace CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Init(InitArgs),
    Build(BuildArgs),
    Dev(DevArgs),
}

#[derive(Debug, clap::Args)]
struct InitArgs {
    #[arg(long, default_value = "openclaw.json")]
    path: PathBuf,
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Language {
    Ts,
    Python,
}

#[derive(Debug, clap::Args)]
struct BuildArgs {
    source: Option<PathBuf>,
    #[arg(long, value_enum)]
    lang: Option<Language>,
    #[arg(long)]
    watch: bool,
    #[arg(long, default_value = "openclaw.json")]
    config: PathBuf,
}

#[derive(Debug, clap::Args)]
struct DevArgs {
    #[arg(long, default_value = "openclaw.json")]
    config: PathBuf,
    #[arg(long, default_value = "8080")]
    port: u16,
}

#[derive(Debug, Clone)]
struct BuildRequest {
    source: PathBuf,
    language: BuildLanguage,
    output_dir: PathBuf,
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Commands::Init(args) => run_init(args),
        Commands::Build(args) => run_build_command(args),
        Commands::Dev(args) => run_dev(args),
    }
}

fn run_init(args: InitArgs) -> Result<(), String> {
    if args.path.exists() && !args.force {
        return Err(format!(
            "{} already exists. Re-run with --force to overwrite it.",
            args.path.display()
        ));
    }

    let default_source = if Path::new("example.claw").exists() {
        PathBuf::from("example.claw")
    } else {
        PathBuf::from("src/pipeline.claw")
    };

    let config = OpenClawConfig::template(default_source);
    config
        .write_pretty(&args.path)
        .map_err(|error| error.to_string())?;

    println!("initialized {}", args.path.display());
    Ok(())
}

fn run_dev(args: DevArgs) -> Result<(), String> {
    let config = OpenClawConfig::load(&args.config).map_err(|error| error.to_string())?;

    // Initial build before starting the gateway
    let build_args = BuildArgs {
        source: None,
        lang: None,
        watch: false,
        config: args.config.clone(),
    };
    let request = resolve_build_request(&build_args, Some(&config))?;
    match run_build_once(&request) {
        Ok(path) => println!("[dev] built {}", path.display()),
        Err(error) => eprintln!("[dev] build error: {error}"),
    }

    // Start the gateway server as a child process
    let gateway_entry = Path::new("openclaw-gateway/src/server.ts");
    if !gateway_entry.exists() {
        return Err(format!(
            "gateway entry point not found at {}",
            gateway_entry.display()
        ));
    }

    println!("[dev] starting gateway on port {}", args.port);
    let mut gateway_child = start_gateway(args.port)?;

    // Set up file watcher for hot-reload
    let (sender, receiver) = mpsc::channel();
    let mut watcher = recommended_watcher(move |result| {
        let _ = sender.send(result);
    })
    .map_err(|error| format!("failed to start file watcher: {error}"))?;

    let mut watched_source = request.source.clone();
    watch_path(&mut watcher, &watched_source)?;
    if args.config.exists() {
        watch_path(&mut watcher, &args.config)?;
    }

    println!(
        "[dev] watching {} for changes (ctrl+c to stop)",
        watched_source.display()
    );

    // Handle ctrl+c to kill the gateway cleanly
    let gateway_pid = gateway_child.id();
    ctrlc::set_handler(move || {
        eprintln!("\n[dev] shutting down...");
        // Best-effort kill via Command since we can't mutably borrow from closure
        let _ = Command::new("kill")
            .arg(gateway_pid.to_string())
            .status();
        std::process::exit(0);
    })
    .map_err(|error| format!("failed to set ctrl+c handler: {error}"))?;

    loop {
        match receiver.recv() {
            Ok(Ok(_event)) => {
                let fresh_config = if args.config.exists() {
                    Some(OpenClawConfig::load(&args.config).map_err(|error| error.to_string())?)
                } else {
                    None
                };

                let fresh_request = resolve_build_request(&build_args, fresh_config.as_ref())?;
                if fresh_request.source != watched_source {
                    watch_path(&mut watcher, &fresh_request.source)?;
                    watched_source = fresh_request.source.clone();
                }

                match run_build_once(&fresh_request) {
                    Ok(path) => println!("[dev] rebuilt {}", path.display()),
                    Err(error) => eprintln!("[dev] build error: {error}"),
                }
            }
            Ok(Err(error)) => eprintln!("[dev] watch error: {error}"),
            Err(error) => {
                let _ = gateway_child.kill();
                return Err(format!("watch channel closed: {error}"));
            }
        }
    }
}

fn start_gateway(port: u16) -> Result<Child, String> {
    Command::new("node")
        .args([
            "--experimental-strip-types",
            "openclaw-gateway/src/server.ts",
        ])
        .env("OPENCLAW_GATEWAY_PORT", port.to_string())
        .spawn()
        .map_err(|error| format!("failed to start gateway: {error}"))
}

fn run_build_command(args: BuildArgs) -> Result<(), String> {
    let config = if args.source.is_none() {
        Some(OpenClawConfig::load(&args.config).map_err(|error| error.to_string())?)
    } else {
        None
    };

    let request = resolve_build_request(&args, config.as_ref())?;
    if args.watch {
        return run_watch_mode(args, request);
    }

    run_build_once(&request).map(|_| ())
}

fn resolve_build_request(
    args: &BuildArgs,
    config: Option<&OpenClawConfig>,
) -> Result<BuildRequest, String> {
    let source = args
        .source
        .clone()
        .or_else(|| config.map(|config| config.build.source.clone()))
        .ok_or_else(|| "no .claw source provided and openclaw.json was not loaded".to_owned())?;

    let language = args
        .lang
        .map(BuildLanguage::from)
        .or_else(|| config.map(|config| config.build.language))
        .unwrap_or(BuildLanguage::Ts);

    let output_dir = config
        .map(|config| config.build.output_dir.clone())
        .unwrap_or_else(|| PathBuf::from("generated/claw"));

    Ok(BuildRequest {
        source,
        language,
        output_dir,
    })
}

fn run_watch_mode(args: BuildArgs, initial_request: BuildRequest) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = recommended_watcher(move |result| {
        let _ = sender.send(result);
    })
    .map_err(|error| format!("failed to start file watcher: {error}"))?;

    let mut watched_source = initial_request.source.clone();
    watch_path(&mut watcher, &watched_source)?;
    if args.config.exists() {
        watch_path(&mut watcher, &args.config)?;
    }

    match run_build_once(&initial_request) {
        Ok(path) => println!("built {}", path.display()),
        Err(error) => eprintln!("{error}"),
    }

    loop {
        match receiver.recv() {
            Ok(Ok(_event)) => {
                let config = if args.source.is_none() && args.config.exists() {
                    Some(OpenClawConfig::load(&args.config).map_err(|error| error.to_string())?)
                } else {
                    None
                };

                let request = resolve_build_request(&args, config.as_ref())?;
                if request.source != watched_source {
                    watch_path(&mut watcher, &request.source)?;
                    watched_source = request.source.clone();
                }

                match run_build_once(&request) {
                    Ok(path) => println!("rebuilt {}", path.display()),
                    Err(error) => eprintln!("{error}"),
                }
            }
            Ok(Err(error)) => eprintln!("watch error: {error}"),
            Err(error) => return Err(format!("watch channel closed: {error}")),
        }
    }
}

fn watch_path(watcher: &mut RecommendedWatcher, path: &Path) -> Result<(), String> {
    watcher
        .watch(path, RecursiveMode::NonRecursive)
        .map_err(|error| format!("failed to watch {}: {error}", path.display()))
}

fn run_build_once(request: &BuildRequest) -> Result<PathBuf, String> {
    let source = read_source_file(&request.source).map_err(|error| error.to_string())?;
    let document = compile_document(&source)
        .map_err(|error| format_compiler_error(&request.source, &source, &error))?;
    let generated = generate_sdk(&document, request.language);
    let output_root = std::env::current_dir().map_err(|source| CompilerError::Io {
        path: PathBuf::from("."),
        source,
    });
    let output_root = output_root.map_err(|error| error.to_string())?;

    write_generated_artifacts(
        &output_root,
        &request.output_dir,
        request.language,
        &document,
        &generated,
    )
        .map_err(|error| error.to_string())
}

fn compile_document(source: &str) -> Result<Document, CompilerError> {
    let document = parser::parse(source)?;
    semantic::analyze(&document)?;
    Ok(document)
}

fn generate_sdk(document: &Document, language: BuildLanguage) -> String {
    match language {
        BuildLanguage::Ts => codegen::generate_ts(document),
        BuildLanguage::Python => codegen::generate_python(document),
    }
    .expect("semantic validation should guarantee code generation succeeds")
}

fn read_source_file(source_path: &Path) -> Result<String, CompilerError> {
    fs::read_to_string(source_path).map_err(|source| CompilerError::Io {
        path: source_path.to_path_buf(),
        source,
    })
}

fn write_generated_artifacts(
    output_root: &Path,
    output_dir: &Path,
    language: BuildLanguage,
    document: &Document,
    generated: &str,
) -> Result<PathBuf, CompilerError> {
    let output_path = write_generated_output(output_root, output_dir, language, generated)?;
    write_compiled_document(output_root, output_dir, document)?;
    Ok(output_path)
}

fn write_generated_output(
    output_root: &Path,
    output_dir: &Path,
    language: BuildLanguage,
    generated: &str,
) -> Result<PathBuf, CompilerError> {
    let output_path = output_root.join(output_dir).join(output_file_name(language));

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|source| CompilerError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    fs::write(&output_path, generated).map_err(|source| CompilerError::Io {
        path: output_path.clone(),
        source,
    })?;

    Ok(output_path)
}

fn write_compiled_document(
    output_root: &Path,
    output_dir: &Path,
    document: &Document,
) -> Result<(), CompilerError> {
    let ast_hash = codegen::document_ast_hash(document);
    let compiled = CompiledDocument {
        ast_hash: ast_hash.clone(),
        document,
    };
    let json = serde_json::to_string_pretty(&compiled).map_err(|source| CompilerError::ParseError {
        message: format!("failed to serialize compiled document: {source}"),
        span: 0..0,
    })?;

    write_text_file(&output_root.join(output_dir).join("document.json"), &json)?;
    write_text_file(
        &output_root.join(output_dir).join(format!("documents/{ast_hash}.json")),
        &json,
    )
}

fn write_text_file(path: &Path, contents: &str) -> Result<(), CompilerError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| CompilerError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    fs::write(path, contents).map_err(|source| CompilerError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn output_file_name(language: BuildLanguage) -> &'static str {
    match language {
        BuildLanguage::Ts => "index.ts",
        BuildLanguage::Python => "__init__.py",
    }
}

#[derive(Serialize)]
struct CompiledDocument<'a> {
    ast_hash: String,
    document: &'a Document,
}

fn format_compiler_error(path: &Path, source: &str, error: &CompilerError) -> String {
    match error {
        CompilerError::Io { .. } => error.to_string(),
        CompilerError::DuplicateSymbol { first_span, .. } => {
            let mut rendered = render_error_with_span(path, source, error);
            let (line, column) = line_and_column(source, first_span.start);
            rendered.push_str(&format!(
                "\nnote: first defined at {}:{}:{}",
                path.display(),
                line,
                column
            ));
            rendered
        }
        _ => render_error_with_span(path, source, error),
    }
}

fn render_error_with_span(path: &Path, source: &str, error: &CompilerError) -> String {
    match error.span() {
        Some(span) => {
            let start = span.start.min(source.len());
            let end = span.end.min(source.len());
            let line_start = line_start(source, start);
            let line_end = line_end(source, start);
            let line_text = &source[line_start..line_end];
            let (line, column) = line_and_column(source, start);
            let caret_offset = source[line_start..start].chars().count();
            let underline_width = if end > start {
                source[start..end.min(line_end)].chars().count().max(1)
            } else {
                1
            };

            format!(
                "error: {error}\n --> {}:{}:{}\n  |\n{:>2} | {}\n  | {}{}",
                path.display(),
                line,
                column,
                line,
                line_text,
                " ".repeat(caret_offset),
                "^".repeat(underline_width)
            )
        }
        None => format!("error: {error}"),
    }
}

fn line_start(source: &str, index: usize) -> usize {
    source[..index].rfind('\n').map_or(0, |offset| offset + 1)
}

fn line_end(source: &str, index: usize) -> usize {
    source[index..]
        .find('\n')
        .map_or(source.len(), |offset| index + offset)
}

fn line_and_column(source: &str, index: usize) -> (usize, usize) {
    let line = source[..index]
        .chars()
        .filter(|character| *character == '\n')
        .count()
        + 1;
    let column = source[line_start(source, index)..index].chars().count() + 1;
    (line, column)
}

impl From<Language> for BuildLanguage {
    fn from(value: Language) -> Self {
        match value {
            Language::Ts => Self::Ts,
            Language::Python => Self::Python,
        }
    }
}

impl From<BuildLanguage> for Language {
    fn from(value: BuildLanguage) -> Self {
        match value {
            BuildLanguage::Ts => Self::Ts,
            BuildLanguage::Python => Self::Python,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_build_request, BuildArgs, Cli, Commands, DevArgs, InitArgs, Language};
    use clap::Parser;
    use clawc::config::{BuildLanguage, OpenClawConfig};
    use std::path::PathBuf;

    #[test]
    fn parses_init_command() {
        let cli = Cli::parse_from(["openclaw", "init", "--force"]);

        match cli.command {
            Commands::Init(InitArgs { force, .. }) => assert!(force),
            _ => panic!("expected init command"),
        }
    }

    #[test]
    fn resolves_build_request_from_config_when_source_is_omitted() {
        let config = OpenClawConfig::template("example.claw");
        let request = resolve_build_request(
            &BuildArgs {
                source: None,
                lang: None,
                watch: false,
                config: PathBuf::from("openclaw.json"),
            },
            Some(&config),
        )
        .unwrap();

        assert_eq!(request.source, PathBuf::from("example.claw"));
        assert_eq!(request.language, BuildLanguage::Ts);
    }

    #[test]
    fn parses_dev_command_with_port() {
        let cli = Cli::parse_from(["openclaw", "dev", "--port", "9090"]);

        match cli.command {
            Commands::Dev(DevArgs { port, .. }) => assert_eq!(port, 9090),
            _ => panic!("expected dev command"),
        }
    }

    #[test]
    fn dev_command_defaults_to_port_8080() {
        let cli = Cli::parse_from(["openclaw", "dev"]);

        match cli.command {
            Commands::Dev(DevArgs { port, .. }) => assert_eq!(port, 8080),
            _ => panic!("expected dev command"),
        }
    }

    #[test]
    fn cli_language_overrides_config_language() {
        let mut config = OpenClawConfig::template("example.claw");
        config.build.language = BuildLanguage::Ts;

        let request = resolve_build_request(
            &BuildArgs {
                source: None,
                lang: Some(Language::Python),
                watch: false,
                config: PathBuf::from("openclaw.json"),
            },
            Some(&config),
        )
        .unwrap();

        assert_eq!(request.language, BuildLanguage::Python);
    }
}
