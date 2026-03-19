pub mod ast;
pub mod codegen;
pub mod config;
pub mod errors;
pub mod lsp;
pub mod parser;
pub mod semantic;
pub mod eval;
#[cfg(test)]
mod parser_tests;
#[cfg(test)]
mod semantic_tests;
#[cfg(test)]
mod codegen_tests;
