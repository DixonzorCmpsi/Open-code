use std::collections::HashMap;

use crate::ast::{Document, ImportDecl, Span};
use crate::errors::{CompilerError, CompilerResult};

const BUILTIN_ERROR_TYPES: [&str; 3] = [
    "AgentExecutionError",
    "SchemaDegradationError",
    "ToolExecutionError",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolKind {
    Type,
    Client,
    Tool,
    Agent,
    Workflow,
}

#[derive(Debug, Clone)]
pub(crate) struct SymbolInfo {
    pub(crate) kind: SymbolKind,
    pub(crate) span: Span,
}

#[derive(Debug, Default)]
pub(crate) struct SymbolTable {
    symbols: HashMap<String, SymbolInfo>,
}

impl SymbolTable {
    pub(crate) fn build(document: &Document) -> CompilerResult<Self> {
        let mut table = Self::default();

        table.register_builtin_error_types()?;
        table.register_imports(&document.imports)?;
        table.register_types(document)?;
        table.register_clients(document)?;
        table.register_tools(document)?;
        table.register_agents(document)?;
        table.register_workflows(document)?;

        Ok(table)
    }

    pub(crate) fn has_type(&self, name: &str) -> bool {
        self.has_kind(name, SymbolKind::Type)
    }

    pub(crate) fn has_client(&self, name: &str) -> bool {
        self.has_kind(name, SymbolKind::Client)
    }

    pub(crate) fn has_tool(&self, name: &str) -> bool {
        self.has_kind(name, SymbolKind::Tool)
    }

    pub(crate) fn has_agent(&self, name: &str) -> bool {
        self.has_kind(name, SymbolKind::Agent)
    }

    fn has_kind(&self, name: &str, kind: SymbolKind) -> bool {
        self.symbols
            .get(name)
            .is_some_and(|symbol| symbol.kind == kind)
    }

    fn register_builtin_error_types(&mut self) -> CompilerResult<()> {
        for name in BUILTIN_ERROR_TYPES {
            self.register(name, SymbolKind::Type, &(0..0))?;
        }

        Ok(())
    }

    fn register_imports(&mut self, imports: &[ImportDecl]) -> CompilerResult<()> {
        for import in imports {
            for name in &import.names {
                self.register(name, SymbolKind::Tool, &import.span)?;
            }
        }

        Ok(())
    }

    fn register_types(&mut self, document: &Document) -> CompilerResult<()> {
        for declaration in &document.types {
            self.register(&declaration.name, SymbolKind::Type, &declaration.span)?;
        }

        Ok(())
    }

    fn register_clients(&mut self, document: &Document) -> CompilerResult<()> {
        for declaration in &document.clients {
            self.register(&declaration.name, SymbolKind::Client, &declaration.span)?;
        }

        Ok(())
    }

    fn register_tools(&mut self, document: &Document) -> CompilerResult<()> {
        for declaration in &document.tools {
            self.register(&declaration.name, SymbolKind::Tool, &declaration.span)?;
        }

        Ok(())
    }

    fn register_agents(&mut self, document: &Document) -> CompilerResult<()> {
        for declaration in &document.agents {
            self.register(&declaration.name, SymbolKind::Agent, &declaration.span)?;
        }

        Ok(())
    }

    fn register_workflows(&mut self, document: &Document) -> CompilerResult<()> {
        for declaration in &document.workflows {
            self.register(&declaration.name, SymbolKind::Workflow, &declaration.span)?;
        }

        Ok(())
    }

    fn register(&mut self, name: &str, kind: SymbolKind, span: &Span) -> CompilerResult<()> {
        if let Some(existing) = self.symbols.get(name) {
            return Err(CompilerError::DuplicateSymbol {
                name: name.to_owned(),
                first_span: existing.span.clone(),
                second_span: span.clone(),
            });
        }

        self.symbols.insert(
            name.to_owned(),
            SymbolInfo {
                kind,
                span: span.clone(),
            },
        );

        Ok(())
    }
}
