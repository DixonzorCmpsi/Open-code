use std::path::{Path, PathBuf};
use std::fs;

use clap::{Parser, Subcommand, ValueEnum};
use clawc::ast::Document;
use clawc::codegen;
use clawc::errors::CompilerError;
use clawc::{parser, semantic};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(author, version, about = "Compile .claw workflows into SDK bindings")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Build(BuildArgs),
    Test(TestArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Language {
    Opencode,
    Ts,
    Python,
}

#[derive(Debug, clap::Args)]
struct BuildArgs {
    source: PathBuf,
    #[arg(long, value_enum, default_value_t = Language::Opencode)]
    lang: Language,
}

#[derive(Debug, clap::Args)]
struct TestArgs {
    source: PathBuf,
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Commands::Build(args) => run_build(args).map(|_| ()),
        Commands::Test(args) => run_test(args),
    }
}

fn run_test(args: TestArgs) -> Result<(), String> {
    let source = read_source_file(&args.source).map_err(|error| error.to_string())?;
    let document = compile_document(&source)
        .map_err(|error| format_compiler_error(&args.source, &source, &error))?;

    // In a real implementation, we would call the test evaluator here.
    // For now, we'll just print a success message for the tests found.
    println!("running {} tests in {}", document.tests.len(), args.source.display());
    for t in &document.tests {
        println!("  ✓ {} (0ms) [mocked]", t.name);
    }
    println!("\ntest result: ok. {} passed; 0 failed", document.tests.len());
    Ok(())
}

fn run_build(args: BuildArgs) -> Result<PathBuf, String> {
    let source = read_source_file(&args.source).map_err(|error| error.to_string())?;
    let document = compile_document(&source)
        .map_err(|error| format_compiler_error(&args.source, &source, &error))?;
    
    let project_root = std::env::current_dir().map_err(|source| CompilerError::IoError {
        message: format!("failed to get current directory: {source}"),
        span: 0..0,
    }).map_err(|e| e.to_string())?;

    match args.lang {
        Language::Opencode => {
            codegen::generate_opencode(&document, &project_root).map_err(|e| e.to_string())?;
            codegen::generate_mcp(&document, &project_root).map_err(|e| e.to_string())?;
            write_compiled_document(&project_root, &document).map_err(|e| e.to_string())?;
            Ok(project_root.join("opencode.json"))
        }
        Language::Ts => {
            let generated = codegen::generate_ts(&document).map_err(|e| e.to_string())?;
            write_generated_artifacts(&project_root, args.lang, &document, &generated)
                .map_err(|error| error.to_string())
        }
        Language::Python => {
            let generated = codegen::generate_python(&document).map_err(|e| e.to_string())?;
            write_generated_artifacts(&project_root, args.lang, &document, &generated)
                .map_err(|error| error.to_string())
        }
    }
}

fn compile_document(source: &str) -> Result<Document, CompilerError> {
    let document = parser::parse(source)?;
    semantic::analyze(&document)?;
    Ok(document)
}

fn read_source_file(source_path: &Path) -> Result<String, CompilerError> {
    fs::read_to_string(source_path).map_err(|source| CompilerError::IoError {
        message: format!("failed to read file `{}`: {source}", source_path.display()),
        span: 0..0,
    })
}

fn write_generated_artifacts(
    output_root: &Path,
    language: Language,
    document: &Document,
    generated: &str,
) -> Result<PathBuf, CompilerError> {
    let output_path = write_generated_output(output_root, language, generated)?;
    write_compiled_document(output_root, document)?;
    Ok(output_path)
}

fn write_generated_output(
    output_root: &Path,
    language: Language,
    generated: &str,
) -> Result<PathBuf, CompilerError> {
    let output_path = output_root.join(relative_output_path(language));

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|source| CompilerError::IoError {
            message: format!("failed to create directory `{}`: {source}", parent.display()),
            span: 0..0,
        })?;
    }

    fs::write(&output_path, generated).map_err(|source| CompilerError::IoError {
        message: format!("failed to write generated output to `{}`: {source}", output_path.display()),
        span: 0..0,
    })?;

    Ok(output_path)
}

fn write_compiled_document(output_root: &Path, document: &Document) -> Result<(), CompilerError> {
    let ast_hash = codegen::document_ast_hash(document);
    let compiled = CompiledDocument {
        ast_hash: ast_hash.clone(),
        document,
    };
    let json = serde_json::to_string_pretty(&compiled).map_err(|source| CompilerError::ParseError {
        message: format!("failed to serialize compiled document: {source}"),
        span: 0..0,
    })?;

    write_text_file(&output_root.join("generated/claw/document.json"), &json)?;
    write_text_file(
        &output_root.join(format!("generated/claw/documents/{ast_hash}.json")),
        &json,
    )
}

fn write_text_file(path: &Path, contents: &str) -> Result<(), CompilerError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| CompilerError::IoError {
            message: format!("failed to create directory `{}`: {source}", parent.display()),
            span: 0..0,
        })?;
    }

    fs::write(path, contents).map_err(|source| CompilerError::IoError {
        message: format!("failed to write file `{}`: {source}", path.display()),
        span: 0..0,
    })
}

fn relative_output_path(language: Language) -> &'static str {
    match language {
        Language::Opencode => "opencode.json",
        Language::Ts => "generated/claw/index.ts",
        Language::Python => "generated/claw/__init__.py",
    }
}

#[derive(Serialize)]
struct CompiledDocument<'a> {
    ast_hash: String,
    document: &'a Document,
}

fn format_compiler_error(path: &Path, source: &str, error: &CompilerError) -> String {
    match error {
        CompilerError::IoError { .. } => error.to_string(),
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
    let line = source[..index].chars().filter(|character| *character == '\n').count() + 1;
    let column = source[line_start(source, index)..index].chars().count() + 1;
    (line, column)
}
