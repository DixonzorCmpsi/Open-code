use std::collections::BTreeSet;

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Diagnostic, DiagnosticSeverity, Position, Range,
    SemanticToken, SemanticTokensLegend, SemanticTokenType,
};

use crate::errors::CompilerError;
use crate::{parser, semantic};

const KEYWORDS: &[&str] = &[
    "agent",
    "assert",
    "break",
    "client",
    "continue",
    "execute",
    "false",
    "for",
    "if",
    "import",
    "in",
    "listener",
    "mock",
    "optional",
    "return",
    "settings",
    "test",
    "tool",
    "true",
    "try",
    "catch",
    "type",
    "workflow",
];

const TOKEN_KEYWORD: u32 = 0;

pub fn diagnostics_for_source(source: &str) -> Vec<Diagnostic> {
    match parser::parse(source) {
        Ok(document) => semantic::analyze_collecting(&document)
            .errors
            .iter()
            .map(|error| compiler_error_to_diagnostic(source, error))
            .collect(),
        Err(error) => vec![compiler_error_to_diagnostic(source, &error)],
    }
}

pub fn completion_items(source: Option<&str>) -> Vec<CompletionItem> {
    let mut labels = BTreeSet::new();
    let mut items = Vec::new();

    for keyword in KEYWORDS {
        labels.insert((*keyword).to_owned());
        items.push(CompletionItem {
            label: (*keyword).to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Claw keyword".to_owned()),
            ..CompletionItem::default()
        });
    }

    if let Some(source) = source {
        if let Ok(document) = parser::parse(source) {
            for label in document
                .types
                .iter()
                .map(|item| (&item.name, CompletionItemKind::STRUCT))
                .chain(document.tools.iter().map(|item| (&item.name, CompletionItemKind::FUNCTION)))
                .chain(document.agents.iter().map(|item| (&item.name, CompletionItemKind::CLASS)))
                .chain(document.workflows.iter().map(|item| (&item.name, CompletionItemKind::FUNCTION)))
                .chain(document.clients.iter().map(|item| (&item.name, CompletionItemKind::MODULE)))
            {
                if labels.insert(label.0.clone()) {
                    items.push(CompletionItem {
                        label: label.0.clone(),
                        kind: Some(label.1),
                        ..CompletionItem::default()
                    });
                }
            }
        }
    }

    items
}

pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![SemanticTokenType::KEYWORD],
        token_modifiers: Vec::new(),
    }
}

pub fn semantic_tokens(source: &str) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let mut previous_line = 0u32;
    let mut previous_start = 0u32;

    for (line_index, line) in source.lines().enumerate() {
        let mut column = 0usize;
        let bytes = line.as_bytes();

        while column < bytes.len() {
            if !bytes[column].is_ascii_alphabetic() {
                column += 1;
                continue;
            }

            let start = column;
            while column < bytes.len()
                && (bytes[column].is_ascii_alphanumeric() || bytes[column] == b'_')
            {
                column += 1;
            }

            let word = &line[start..column];
            if !KEYWORDS.contains(&word) {
                continue;
            }

            let line_index = line_index as u32;
            let start = start as u32;
            let delta_line = line_index - previous_line;
            let delta_start = if delta_line == 0 {
                start - previous_start
            } else {
                start
            };

            tokens.push(SemanticToken {
                delta_line,
                delta_start,
                length: word.len() as u32,
                token_type: TOKEN_KEYWORD,
                token_modifiers_bitset: 0,
            });

            previous_line = line_index;
            previous_start = start;
        }
    }

    tokens
}

fn compiler_error_to_diagnostic(source: &str, error: &CompilerError) -> Diagnostic {
    let range = error
        .span()
        .map(|span| span_to_range(source, span.start, span.end))
        .unwrap_or_else(|| Range::new(Position::new(0, 0), Position::new(0, 0)));

    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("claw-lsp".to_owned()),
        message: error.to_string(),
        ..Diagnostic::default()
    }
}

fn span_to_range(source: &str, start: usize, end: usize) -> Range {
    let start_offset = start.min(source.len());
    let end_offset = end.max(start_offset).min(source.len());
    Range::new(
        offset_to_position(source, start_offset),
        offset_to_position(source, end_offset),
    )
}

fn offset_to_position(source: &str, offset: usize) -> Position {
    let bounded = offset.min(source.len());
    let line = source[..bounded].chars().filter(|character| *character == '\n').count() as u32;
    let column_start = source[..bounded].rfind('\n').map_or(0, |index| index + 1);
    let column = source[column_start..bounded].chars().count() as u32;
    Position::new(line, column)
}

#[cfg(test)]
mod tests {
    use super::{completion_items, diagnostics_for_source, semantic_tokens};

    #[test]
    fn reports_parse_diagnostics_for_invalid_source() {
        let diagnostics = diagnostics_for_source("workflow Demo(\n");

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("parse error"));
    }

    #[test]
    fn completions_include_keywords_and_document_symbols() {
        let source = r#"
            type SearchResult {
                url: string
            }

            workflow Analyze(company: string) -> SearchResult {
                return "noop"
            }
        "#;

        let items = completion_items(Some(source));
        let labels = items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>();

        assert!(labels.contains(&"workflow"));
        assert!(labels.contains(&"SearchResult"));
        assert!(labels.contains(&"Analyze"));
    }

    #[test]
    fn semantic_tokens_include_keyword_entries() {
        let tokens = semantic_tokens(
            r#"
            workflow Analyze(company: string) -> string {
                return "ok"
            }
        "#,
        );

        assert!(!tokens.is_empty());
    }

    #[test]
    fn diagnostics_report_multiple_semantic_errors() {
        let diagnostics = diagnostics_for_source(
            r#"
            client FastOpenAI {
                provider = "openai"
                model = "gpt-5.1"
            }

            agent Researcher {
                client = MissingClient
                tools = [MissingTool]
            }

            workflow Analyze(company: string) -> string {
                let score: int = "oops"
            }
        "#,
        );

        assert_eq!(diagnostics.len(), 4);
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.message.contains("undefined tool")));
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.message.contains("undefined client")));
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.message.contains("type mismatch")));
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.message.contains("missing return")));
    }
}
