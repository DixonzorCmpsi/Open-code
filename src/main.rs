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
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Language {
    Ts,
    Python,
}

#[derive(Debug, clap::Args)]
struct BuildArgs {
    source: PathBuf,
    #[arg(long, value_enum, default_value_t = Language::Ts)]
    lang: Language,
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
    }
}

fn run_build(args: BuildArgs) -> Result<PathBuf, String> {
    let source = read_source_file(&args.source).map_err(|error| error.to_string())?;
    let document = compile_document(&source)
        .map_err(|error| format_compiler_error(&args.source, &source, &error))?;
    let generated = generate_sdk(&document, args.lang);
    let output_root = std::env::current_dir().map_err(|source| CompilerError::Io {
        path: PathBuf::from("."),
        source,
    });
    let output_root = output_root.map_err(|error| error.to_string())?;

    write_generated_artifacts(&output_root, args.lang, &document, &generated)
        .map_err(|error| error.to_string())
}

fn compile_document(source: &str) -> Result<Document, CompilerError> {
    let document = parser::parse(source)?;
    semantic::analyze(&document)?;
    Ok(document)
}

fn generate_sdk(document: &Document, language: Language) -> String {
    match language {
        Language::Ts => codegen::generate_ts(document),
        Language::Python => codegen::generate_python(document),
    }
    .expect("semantic validation should guarantee code generation succeeds")
}

#[cfg(test)]
fn compile_source_to_typescript(source: &str) -> Result<String, CompilerError> {
    let document = compile_document(source)?;
    codegen::generate_ts(&document)
}

#[cfg(test)]
fn compile_source_to_python(source: &str) -> Result<String, CompilerError> {
    let document = compile_document(source)?;
    codegen::generate_python(&document)
}

#[cfg(test)]
fn build_into_directory(
    source_path: &Path,
    output_root: &Path,
    language: Language,
) -> Result<PathBuf, CompilerError> {
    let source = read_source_file(source_path)?;
    let document = compile_document(&source)?;
    let generated = generate_sdk(&document, language);
    write_generated_artifacts(output_root, language, &document, &generated)
}

fn read_source_file(source_path: &Path) -> Result<String, CompilerError> {
    fs::read_to_string(source_path).map_err(|source| CompilerError::Io {
        path: source_path.to_path_buf(),
        source,
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

fn relative_output_path(language: Language) -> &'static str {
    match language {
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
    let line = source[..index].chars().filter(|character| *character == '\n').count() + 1;
    let column = source[line_start(source, index)..index].chars().count() + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::{
        build_into_directory, compile_source_to_python, compile_source_to_typescript,
        format_compiler_error, Cli, Commands, Language,
    };
    use clap::Parser;
    use clawc::errors::CompilerError;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_build_command_with_default_typescript_language() {
        let cli = Cli::parse_from(["clawc", "build", "example.claw"]);

        match cli.command {
            Commands::Build(args) => {
                assert_eq!(args.source, PathBuf::from("example.claw"));
                assert_eq!(args.lang, Language::Ts);
            }
        }
    }

    #[test]
    fn parses_build_command_with_python_language() {
        let cli = Cli::parse_from(["clawc", "build", "example.claw", "--lang", "python"]);

        match cli.command {
            Commands::Build(args) => {
                assert_eq!(args.source, PathBuf::from("example.claw"));
                assert_eq!(args.lang, Language::Python);
            }
        }
    }

    #[test]
    fn compiles_valid_source_through_the_full_pipeline() {
        let output = compile_source_to_typescript(valid_source()).unwrap();

        assert!(output.contains("export interface SearchResult"));
        assert!(output.contains("export const AnalyzeCompetitors = async ("));
        assert!(output.contains("return SearchResultSchema.parse(result);"));
    }

    #[test]
    fn writes_generated_sdk_to_generated_claw_index_ts() {
        let temp_root = temp_test_dir();
        let source_path = temp_root.join("example.claw");
        fs::write(&source_path, valid_source()).unwrap();

        let output_path = build_into_directory(&source_path, &temp_root, Language::Ts).unwrap();
        let generated = fs::read_to_string(&output_path).unwrap();

        assert_eq!(output_path, temp_root.join("generated/claw/index.ts"));
        assert!(generated.contains(r#"workflowName: "AnalyzeCompetitors""#));

        fs::remove_dir_all(&temp_root).unwrap();
    }

    #[test]
    fn writes_compiled_document_json_for_gateway_execution() {
        let temp_root = temp_test_dir();
        let source_path = temp_root.join("example.claw");
        fs::write(&source_path, valid_source()).unwrap();

        build_into_directory(&source_path, &temp_root, Language::Ts).unwrap();

        let manifest_path = temp_root.join("generated/claw/document.json");
        let manifest = fs::read_to_string(&manifest_path).unwrap();

        assert!(manifest.contains(r#""ast_hash""#));
        assert!(manifest.contains(r#""workflows""#));
        assert!(manifest.contains(r#""AnalyzeCompetitors""#));

        fs::remove_dir_all(&temp_root).unwrap();
    }

    #[test]
    fn writes_hash_named_compiled_document_snapshot() {
        let temp_root = temp_test_dir();
        let source_path = temp_root.join("example.claw");
        fs::write(&source_path, valid_source()).unwrap();

        build_into_directory(&source_path, &temp_root, Language::Ts).unwrap();

        let documents_dir = temp_root.join("generated/claw/documents");
        let entries = fs::read_dir(&documents_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(entries.len(), 1);
        assert!(entries[0].ends_with(".json"));

        fs::remove_dir_all(&temp_root).unwrap();
    }

    #[test]
    fn compiles_valid_source_to_python_sdk() {
        let output = compile_source_to_python(valid_source()).unwrap();

        assert!(output.contains("class SearchResult(BaseModel):"));
        assert!(output.contains("async def analyze_competitors("));
        assert!(output.contains("ast_hash=CLAW_AST_HASH"));
    }

    #[test]
    fn writes_generated_python_sdk_to_generated_claw_init_py() {
        let temp_root = temp_test_dir();
        let source_path = temp_root.join("example.claw");
        fs::write(&source_path, valid_source()).unwrap();

        let output_path = build_into_directory(&source_path, &temp_root, Language::Python).unwrap();
        let generated = fs::read_to_string(&output_path).unwrap();

        assert_eq!(output_path, temp_root.join("generated/claw/__init__.py"));
        assert!(generated.contains("async def analyze_competitors("));

        fs::remove_dir_all(&temp_root).unwrap();
    }

    #[test]
    fn formats_span_carrying_errors_with_source_context() {
        let source = "workflow Analyze() { return }\n";
        let error = CompilerError::ParseError {
            message: "expected expression".to_owned(),
            span: 28..28,
        };
        let rendered = format_compiler_error(PathBuf::from("example.claw").as_path(), source, &error);

        assert!(rendered.contains("error: parse error"));
        assert!(rendered.contains("--> example.claw:1:29"));
        assert!(rendered.contains("^"));
    }

    fn valid_source() -> &'static str {
        r#"
            client FastOpenAI {
                provider = "openai"
                model = "gpt-5.1"
                retries = 3
            }

            type SearchResult {
                url: string @regex("^https://")
                confidence_score: float @min(0)
                snippet: string
                tags: list<string>
            }

            tool WebSearch(query: string) -> SearchResult {
                invoke: module("scripts.search").function("run")
            }

            agent Researcher {
                client = FastOpenAI
                system_prompt = "Stay deterministic."
                tools = [WebSearch]
                settings = {
                    max_steps: 5,
                    temperature: 0.1
                }
            }

            workflow AnalyzeCompetitors(company: string) -> SearchResult {
                let report: SearchResult = execute Researcher.run(
                    task: company,
                    require_type: SearchResult,
                )
                return report
            }
        "#
    }

    fn temp_test_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("clawc-cli-test-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
