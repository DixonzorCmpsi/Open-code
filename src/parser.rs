use winnow::ascii::digit1;
use winnow::combinator::{alt, cut_err, delimited, eof, opt, preceded, repeat, separated, terminated};
use winnow::error::{ContextError, ErrMode, ParseError as WinnowParseError};
use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow::token::{take_till, take_while};

use crate::ast::{
    AgentDecl, AgentSetting, AgentSettings, BinaryOp, Block, ClientDecl, Constraint, DataType,
    Document, ElseBranch, ExpectOp, Expr, ImportDecl, ListenerDecl, MockDecl, SettingValue, SpannedExpr,
    Statement, SynthesizerDecl, TestBlock, TestDecl, ToolDecl, TypeDecl, TypeField, UsingExpr, WorkflowDecl,
};
use crate::errors::{CompilerError, CompilerResult};

type Input<'a> = LocatingSlice<&'a str>;
type PResult<T> = winnow::ModalResult<T, ContextError>;
type ExecuteKwargs = Vec<(String, SpannedExpr)>;
type ExecuteRunParts = (String, ExecuteKwargs, Option<DataType>);

enum Declaration {
    Import(ImportDecl),
    Type(TypeDecl),
    Client(ClientDecl),
    Synthesizer(SynthesizerDecl),
    Tool(ToolDecl),
    Agent(AgentDecl),
    Workflow(WorkflowDecl),
    Listener(ListenerDecl),
    Test(TestDecl),
    Mock(MockDecl),
}

enum ClientSettingValue {
    Provider(String),
    Model(String),
    Retries(u32),
    Timeout(u32),
    Endpoint(SpannedExpr),
    ApiKey(SpannedExpr),
}

enum SynthesizerSettingValue {
    Client(String),
    Temperature(f64),
    MaxTokens(u64),
}

enum AgentProperty {
    Client(String),
    SystemPrompt(String),
    Tools(Vec<String>),
    Settings(AgentSettings),
}

enum ExecuteArgument {
    Kwarg(String, SpannedExpr),
    RequireType(DataType),
}

pub fn parse_document(source: &str) -> CompilerResult<Document> {
    parse_complete(source, document)
}

pub fn parse(source: &str) -> CompilerResult<Document> {
    parse_document(source)
}

#[cfg(test)]
fn parse_identifier(source: &str) -> CompilerResult<String> {
    parse_complete(source, identifier)
}

#[cfg(test)]
fn parse_string_literal(source: &str) -> CompilerResult<String> {
    parse_complete(source, string_literal)
}

#[cfg(test)]
fn parse_data_type(source: &str) -> CompilerResult<DataType> {
    parse_complete(source, data_type)
}

#[cfg(test)]
fn parse_type_decl(source: &str) -> CompilerResult<TypeDecl> {
    parse_complete(source, type_decl)
}

#[cfg(test)]
fn parse_tool_decl(source: &str) -> CompilerResult<ToolDecl> {
    parse_complete(source, tool_decl)
}

#[cfg(test)]
fn parse_agent_decl(source: &str) -> CompilerResult<AgentDecl> {
    parse_complete(source, agent_decl)
}

#[cfg(test)]
fn parse_workflow_decl(source: &str) -> CompilerResult<WorkflowDecl> {
    parse_complete(source, workflow_decl)
}

fn parse_complete<'a, T>(
    source: &'a str,
    mut parser: impl Parser<Input<'a>, T, ErrMode<ContextError>>,
) -> CompilerResult<T> {
    parser
        .parse(Input::new(source))
        .map_err(map_parse_error)
}

fn map_parse_error(error: WinnowParseError<Input<'_>, ContextError>) -> CompilerError {
    CompilerError::ParseError {
        message: error.to_string(),
        span: error.char_span(),
    }
}

fn document(input: &mut Input<'_>) -> PResult<Document> {
    let mut parser = preceded(
        trivia,
        terminated(
            repeat(0.., declaration).map(|declarations: Vec<Declaration>| {
                let mut document = Document {
                    imports: Vec::new(),
                    types: Vec::new(),
                    clients: Vec::new(),
                    tools: Vec::new(),
                    agents: Vec::new(),
                    workflows: Vec::new(),
                    listeners: Vec::new(),
                    tests: Vec::new(),
                    mocks: Vec::new(),
                    synthesizers: Vec::new(),
                    span: 0..0,
                };

                for declaration in declarations {
                    match declaration {
                        Declaration::Import(declaration) => document.imports.push(declaration),
                        Declaration::Type(declaration) => document.types.push(declaration),
                        Declaration::Client(declaration) => document.clients.push(declaration),
                        Declaration::Synthesizer(declaration) => document.synthesizers.push(declaration),
                        Declaration::Tool(declaration) => document.tools.push(declaration),
                        Declaration::Agent(declaration) => document.agents.push(declaration),
                        Declaration::Workflow(declaration) => document.workflows.push(declaration),
                        Declaration::Listener(declaration) => document.listeners.push(declaration),
                        Declaration::Test(declaration) => document.tests.push(declaration),
                        Declaration::Mock(declaration) => document.mocks.push(declaration),
                    }
                }

                document
            })
            .with_span(),
            eof,
        ),
    );

    let (mut document, span) = parser.parse_next(input)?;
    document.span = span;
    Ok(document)
}

fn declaration(input: &mut Input<'_>) -> PResult<Declaration> {
    alt((
        import_decl.map(Declaration::Import),
        type_decl.map(Declaration::Type),
        client_decl.map(Declaration::Client),
        synthesizer_decl.map(Declaration::Synthesizer),
        tool_decl.map(Declaration::Tool),
        agent_decl.map(Declaration::Agent),
        workflow_decl.map(Declaration::Workflow),
        listener_decl.map(Declaration::Listener),
        test_decl.map(Declaration::Test),
        mock_decl.map(Declaration::Mock),
    ))
    .parse_next(input)
}

fn import_decl(input: &mut Input<'_>) -> PResult<ImportDecl> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "import",
                brace_delimited(comma_separated0(simple_identifier_raw)),
                "from",
                string_literal,
            )
                .with_span()
                .map(|((_, names, _, source), span)| ImportDecl { names, source, span }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn type_decl(input: &mut Input<'_>) -> PResult<TypeDecl> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "type",
                lexeme(simple_identifier_raw),
                brace_delimited(repeat(1.., type_field)),
            )
                .with_span()
                .map(|((_, name, fields), span)| TypeDecl { name, fields, span }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn type_field(input: &mut Input<'_>) -> PResult<TypeField> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                simple_identifier_raw,
                lexeme(':'),
                raw_data_type,
                repeat(0.., constraint),
            )
                .with_span()
                .map(|((name, _, data_type, constraints), span)| TypeField {
                    name,
                    data_type,
                    constraints,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn constraint(input: &mut Input<'_>) -> PResult<Constraint> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                '@',
                simple_identifier_raw,
                paren_delimited(alt((raw_string_expr, raw_number_expr)).with_span().map(
                    |(expr, span)| SpannedExpr { expr, span },
                )),
            )
                .with_span()
                .map(|((_, name, value), span)| Constraint {
                    name,
                    value,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn client_decl(input: &mut Input<'_>) -> PResult<ClientDecl> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "client",
                lexeme(simple_identifier_raw),
                brace_delimited(repeat::<_, _, Vec<ClientSettingValue>, _, _>(
                    1..,
                    client_setting,
                )),
            )
                .with_span()
                .map(|((_, name, settings), span)| {
                    let mut declaration = ClientDecl {
                        name,
                        provider: String::new(),
                        model: String::new(),
                        retries: None,
                        timeout_ms: None,
                        endpoint: None,
                        api_key: None,
                        span,
                    };

                    for setting in settings {
                        match setting {
                            ClientSettingValue::Provider(value) => declaration.provider = value,
                            ClientSettingValue::Model(value) => declaration.model = value,
                            ClientSettingValue::Retries(value) => declaration.retries = Some(value),
                            ClientSettingValue::Timeout(value) => {
                                declaration.timeout_ms = Some(value)
                            }
                            ClientSettingValue::Endpoint(value) => {
                                declaration.endpoint = Some(value)
                            }
                            ClientSettingValue::ApiKey(value) => declaration.api_key = Some(value),
                        }
                    }

                    declaration
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn client_setting(input: &mut Input<'_>) -> PResult<ClientSettingValue> {
    let key = lexeme(simple_identifier_raw).parse_next(input)?;
    lexeme('=').parse_next(input)?;

    match key.as_str() {
        "provider" => string_literal
            .map(ClientSettingValue::Provider)
            .parse_next(input),
        "model" => string_literal.map(ClientSettingValue::Model).parse_next(input),
        "retries" => integer_literal_u32
            .map(ClientSettingValue::Retries)
            .parse_next(input),
        "timeout" => integer_literal_u32
            .map(ClientSettingValue::Timeout)
            .parse_next(input),
        "endpoint" => expr.map(ClientSettingValue::Endpoint).parse_next(input),
        "api_key" => expr.map(ClientSettingValue::ApiKey).parse_next(input),
        _ => unreachable!("client_setting_key grammar restricts this match"),
    }
}

fn synthesizer_decl(input: &mut Input<'_>) -> PResult<SynthesizerDecl> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "synthesizer",
                lexeme(simple_identifier_raw),
                brace_delimited(repeat::<_, _, Vec<SynthesizerSettingValue>, _, _>(
                    1..,
                    synthesizer_setting,
                )),
            )
                .with_span()
                .map(|((_, name, settings), span)| {
                    let mut declaration = SynthesizerDecl {
                        name,
                        client: String::new(),
                        temperature: None,
                        max_tokens: None,
                        span,
                    };

                    for setting in settings {
                        match setting {
                            SynthesizerSettingValue::Client(value) => declaration.client = value,
                            SynthesizerSettingValue::Temperature(value) => declaration.temperature = Some(value),
                            SynthesizerSettingValue::MaxTokens(value) => declaration.max_tokens = Some(value),
                        }
                    }

                    declaration
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn synthesizer_setting(input: &mut Input<'_>) -> PResult<SynthesizerSettingValue> {
    let key = lexeme(simple_identifier_raw).parse_next(input)?;
    lexeme('=').parse_next(input)?;

    match key.as_str() {
        "client" => simple_identifier_raw
            .map(SynthesizerSettingValue::Client)
            .parse_next(input),
        "temperature" => raw_number_expr
            .map(|e| match e {
                Expr::FloatLiteral(f) => SynthesizerSettingValue::Temperature(f),
                Expr::IntLiteral(i) => SynthesizerSettingValue::Temperature(i as f64),
                _ => unreachable!(),
            })
            .parse_next(input),
        "max_tokens" => integer_literal_u32
            .map(|e| SynthesizerSettingValue::MaxTokens(e as u64))
            .parse_next(input),
        _ => Err(ErrMode::from_input(input)),
    }
}

enum ToolProperty {
    Invoke(String),
    Using(UsingExpr),
    Synthesizer(String),
    Test(TestBlock),
}

fn tool_decl(input: &mut Input<'_>) -> PResult<ToolDecl> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "tool",
                lexeme(simple_identifier_raw),
                paren_delimited(opt(tool_args)),
                opt(preceded(lexeme("->"), raw_data_type)),
                opt(brace_delimited(repeat::<_, _, Vec<ToolProperty>, _, _>(
                    0..,
                    tool_property_parser,
                ))),
            )
                .with_span()
                .verify_map(|((_, name, arguments, return_type, opt_props), span)| {
                    let mut decl = ToolDecl {
                        name,
                        arguments: arguments.unwrap_or_default(),
                        return_type,
                        invoke_path: None,
                        using: None,
                        synthesizer: None,
                        test_block: None,
                        span: span.clone(),
                    };

                    if let Some(props) = opt_props {
                        for prop in props {
                            match prop {
                                ToolProperty::Invoke(path) => decl.invoke_path = Some(path),
                                ToolProperty::Using(u) => decl.using = Some(u),
                                ToolProperty::Synthesizer(s) => decl.synthesizer = Some(s),
                                ToolProperty::Test(t) => decl.test_block = Some(t),
                            }
                        }
                    }

                    if decl.invoke_path.is_some() && decl.using.is_some() {
                        return None;
                    }

                    Some(decl)
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn tool_args(input: &mut Input<'_>) -> PResult<Vec<TypeField>> {
    comma_separated0(type_field).parse_next(input)
}

fn tool_property_parser(input: &mut Input<'_>) -> PResult<ToolProperty> {
    let key = lexeme(simple_identifier_raw).parse_next(input)?;

    match key.as_str() {
        "invoke" => preceded(
            lexeme(':'),
            // read rest of line essentially, or until newline. For backwards compat with tests:
            take_till(0.., ('\n', '}')).map(|path: &str| ToolProperty::Invoke(path.trim().to_owned())),
        )
        .parse_next(input),
        "using" => preceded(
            lexeme(':'),
            using_expr,
        )
        .map(ToolProperty::Using)
        .parse_next(input),
        "synthesizer" => preceded(
            lexeme(':'),
            lexeme(simple_identifier_raw),
        )
        .map(ToolProperty::Synthesizer)
        .parse_next(input),
        "test" => test_body.map(ToolProperty::Test).parse_next(input),
        _ => Err(ErrMode::from_input(input)),
    }
}

fn using_expr(input: &mut Input<'_>) -> PResult<UsingExpr> {
    alt((
        lexeme("fetch").map(|_| UsingExpr::Fetch),
        lexeme("playwright").map(|_| UsingExpr::Playwright),
        lexeme("bash").map(|_| UsingExpr::Bash),
        preceded(lexeme("mcp"), paren_delimited(string_literal))
            .map(UsingExpr::Mcp),
        preceded(lexeme("baml"), paren_delimited(string_literal))
            .map(UsingExpr::Baml),
    ))
    .parse_next(input)
}

fn test_body(input: &mut Input<'_>) -> PResult<TestBlock> {
    brace_delimited(test_body_inner).parse_next(input)
}

fn test_body_inner(input: &mut Input<'_>) -> PResult<TestBlock> {
    let mut inputs = vec![];
    let mut expects = vec![];

    // it should be input: { ... } \n expect: { ... }
    let fields = repeat::<_, _, Vec<(String, TestBlockField)>, _, _>(
        0..,
        test_block_field
    ).parse_next(input)?;

    for (k, v) in fields {
        match k.as_str() {
            "input" => {
                if let TestBlockField::Input(i) = v { inputs = i; }
            }
            "expect" => {
                if let TestBlockField::Expect(e) = v { expects = e; }
            }
            _ => {}
        }
    }

    Ok(TestBlock { input: inputs, expect: expects })
}

enum TestBlockField {
    Input(Vec<(String, SpannedExpr)>),
    Expect(Vec<(String, ExpectOp)>),
}

fn test_block_field(input: &mut Input<'_>) -> PResult<(String, TestBlockField)> {
    let key = lexeme(simple_identifier_raw).parse_next(input)?;
    lexeme(':').parse_next(input)?;

    match key.as_str() {
        "input" => {
            let kvs = brace_delimited(comma_separated0(test_input_kv)).parse_next(input)?;
            Ok((key, TestBlockField::Input(kvs)))
        }
        "expect" => {
            let kvs = brace_delimited(comma_separated0(test_expect_kv)).parse_next(input)?;
            Ok((key, TestBlockField::Expect(kvs)))
        }
        _ => Err(ErrMode::from_input(input))
    }
}

fn test_input_kv(input: &mut Input<'_>) -> PResult<(String, SpannedExpr)> {
    let k = lexeme(simple_identifier_raw).parse_next(input)?;
    let v = preceded(lexeme(':'), expr).parse_next(input)?;
    Ok((k, v))
}

fn test_expect_kv(input: &mut Input<'_>) -> PResult<(String, ExpectOp)> {
    let k = lexeme(simple_identifier_raw).parse_next(input)?;
    let v = preceded(lexeme(':'), expect_op).parse_next(input)?;
    Ok((k, v))
}

fn expect_op(input: &mut Input<'_>) -> PResult<ExpectOp> {
    alt((
        lexeme("!empty").map(|_| ExpectOp::NotEmpty),
        preceded(lexeme(">="), number_f64).map(ExpectOp::Gte),
        preceded(lexeme("<="), number_f64).map(ExpectOp::Lte),
        preceded(lexeme(">"), number_f64).map(ExpectOp::Gt),
        preceded(lexeme("<"), number_f64).map(ExpectOp::Lt),
        preceded(lexeme("=="), expr).map(ExpectOp::Eq),
        preceded(lexeme("matches"), string_literal).map(ExpectOp::Matches),
    )).parse_next(input)
}

fn number_f64(input: &mut Input<'_>) -> PResult<f64> {
    raw_number_expr.map(|e| match e {
        Expr::FloatLiteral(f) => f,
        Expr::IntLiteral(i) => i as f64,
        _ => unreachable!(),
    }).parse_next(input)
}

fn agent_decl(input: &mut Input<'_>) -> PResult<AgentDecl> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "agent",
                lexeme(simple_identifier_raw),
                opt(preceded(
                    lexeme("extends"),
                    lexeme(simple_identifier_raw),
                )),
                brace_delimited(repeat::<_, _, Vec<AgentProperty>, _, _>(
                    1..,
                    agent_property,
                )),
            )
                .with_span()
                .map(|((_, name, extends, properties), span)| {
                    let mut declaration = AgentDecl {
                        name,
                        extends,
                        client: None,
                        system_prompt: None,
                        tools: Vec::new(),
                        settings: AgentSettings {
                            entries: Vec::new(),
                            span: span.clone(),
                        },
                        dynamic_reasoning: std::cell::Cell::new(false),
                        span,
                    };

                    for property in properties {
                        match property {
                            AgentProperty::Client(value) => declaration.client = Some(value),
                            AgentProperty::SystemPrompt(value) => {
                                declaration.system_prompt = Some(value)
                            }
                            AgentProperty::Tools(mut value) => declaration.tools.append(&mut value),
                            AgentProperty::Settings(value) => declaration.settings = value,
                        }
                    }

                    declaration
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn agent_property(input: &mut Input<'_>) -> PResult<AgentProperty> {
    let key = lexeme(simple_identifier_raw).parse_next(input)?;

    match key.as_str() {
        "client" => preceded(
            lexeme('='),
            lexeme(simple_identifier_raw).map(AgentProperty::Client),
        )
        .parse_next(input),
        "system_prompt" => preceded(
            lexeme('='),
            string_literal.map(AgentProperty::SystemPrompt),
        )
        .parse_next(input),
        "tools" => preceded(
            alt((lexeme("+="), lexeme("="))),
            bracket_delimited(comma_separated0(scoped_identifier)),
        )
        .map(AgentProperty::Tools)
        .parse_next(input),
        "settings" => preceded(lexeme('='), settings_block.map(AgentProperty::Settings)).parse_next(input),
        _ => unreachable!("agent_prop grammar restricts this match"),
    }
}

fn settings_block(input: &mut Input<'_>) -> PResult<AgentSettings> {
    let mut parser = preceded(
        trivia,
        terminated(
            delimited(
                '{',
                terminated(repeat(1.., setting_entry), opt(lexeme(','))),
                cut_err(preceded(trivia, '}')),
            )
                .with_span()
                .map(|(entries, span)| AgentSettings { entries, span }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn setting_entry(input: &mut Input<'_>) -> PResult<AgentSetting> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                simple_identifier_raw,
                lexeme(':'),
                setting_value,
                opt(lexeme(',')),
            )
                .with_span()
                .map(|((name, _, value, _), span)| AgentSetting {
                    name,
                    value,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn setting_value(input: &mut Input<'_>) -> PResult<SettingValue> {
    alt((
        raw_number_expr.map(|value| match value {
            Expr::IntLiteral(value) => SettingValue::Int(value),
            Expr::FloatLiteral(value) => SettingValue::Float(value),
            _ => unreachable!("raw_number_expr only returns numeric literals"),
        }),
        raw_bool_expr.map(|value| match value {
            Expr::BoolLiteral(value) => SettingValue::Boolean(value),
            _ => unreachable!("raw_bool_expr only returns booleans"),
        }),
    ))
    .parse_next(input)
}

fn workflow_decl(input: &mut Input<'_>) -> PResult<WorkflowDecl> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "workflow",
                lexeme(simple_identifier_raw),
                paren_delimited(opt(tool_args)),
                opt(preceded(lexeme("->"), raw_data_type)),
                block,
            )
                .with_span()
                .map(|((_, name, arguments, return_type, body), span)| WorkflowDecl {
                    name,
                    arguments: arguments.unwrap_or_default(),
                    return_type,
                    body,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn listener_decl(input: &mut Input<'_>) -> PResult<ListenerDecl> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "listener",
                lexeme(simple_identifier_raw),
                paren_delimited(("event", lexeme(':'), scoped_identifier)),
                block,
            )
                .with_span()
                .map(|((_, name, (_, _, event_type), body), span)| ListenerDecl {
                    name,
                    event_type,
                    body,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn test_decl(input: &mut Input<'_>) -> PResult<TestDecl> {
    let mut parser = preceded(
        trivia,
        terminated(
            ("test", string_literal, block)
                .with_span()
                .map(|((_, name, body), span)| TestDecl { name, body, span }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn mock_decl(input: &mut Input<'_>) -> PResult<MockDecl> {
    let mock_entry = (
        lexeme(simple_identifier_raw),
        lexeme(':'),
        expr,
        opt(lexeme(',')),
    )
        .map(|(name, _, value, _)| (name, value));

    let mut parser = preceded(
        trivia,
        terminated(
            (
                "mock",
                lexeme(simple_identifier_raw),
                brace_delimited(repeat(1.., mock_entry)),
            )
                .with_span()
                .map(|((_, target_agent, output), span)| MockDecl {
                    target_agent,
                    output,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn block(input: &mut Input<'_>) -> PResult<Block> {
    let mut parser = preceded(
        trivia,
        terminated(
            delimited('{', repeat(0.., statement), cut_err(preceded(trivia, '}')))
                .with_span()
                .map(|(statements, span)| Block { statements, span }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn statement(input: &mut Input<'_>) -> PResult<Statement> {
    alt((
        let_stmt,
        for_stmt,
        if_stmt,
        try_stmt,
        return_stmt,
        execute_stmt,
        continue_stmt,
        break_stmt,
        assert_stmt,
        reason_stmt,
        expression_stmt,
    ))
    .parse_next(input)
}

fn let_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "let",
                lexeme(simple_identifier_raw),
                opt(preceded(lexeme(':'), raw_data_type)),
                lexeme('='),
                expr,
            )
                .with_span()
                .map(|((_, name, explicit_type, _, value), span)| Statement::LetDecl {
                    name,
                    explicit_type,
                    value,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

enum ReasonField {
    Using(String),
    Input(String),
    Goal(String),
    OutputType(DataType),
    Bind(String),
}

fn reason_field(input: &mut Input<'_>) -> PResult<(String, ReasonField)> {
    let key = lexeme(simple_identifier_raw).parse_next(input)?;
    lexeme(':').parse_next(input)?;

    match key.as_str() {
        "using" => simple_identifier_raw.map(ReasonField::Using).parse_next(input).map(|v| (key, v)),
        "input" => simple_identifier_raw.map(ReasonField::Input).parse_next(input).map(|v| (key, v)),
        "goal" => string_literal.map(ReasonField::Goal).parse_next(input).map(|v| (key, v)),
        "output_type" => raw_data_type.map(ReasonField::OutputType).parse_next(input).map(|v| (key, v)),
        "bind" => simple_identifier_raw.map(ReasonField::Bind).parse_next(input).map(|v| (key, v)),
        _ => Err(ErrMode::from_input(input)),
    }
}

fn reason_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "reason",
                brace_delimited(repeat::<_, _, Vec<(String, ReasonField)>, _, _>(
                    1..,
                    reason_field
                ))
            ).with_span().verify_map(|((_, fields), span)| {
                let mut using_agent = None;
                let mut input_var = None;
                let mut goal = None;
                let mut output_type = None;
                let mut bind = None;

                for (_k, v) in fields {
                    match v {
                        ReasonField::Using(u) => using_agent = Some(u),
                        ReasonField::Input(i) => input_var = Some(i),
                        ReasonField::Goal(g) => goal = Some(g),
                        ReasonField::OutputType(t) => output_type = Some(t),
                        ReasonField::Bind(b) => bind = Some(b),
                    }
                }

                if using_agent.is_none() || input_var.is_none() || goal.is_none() || output_type.is_none() || bind.is_none() {
                    return None;
                }

                Some(Statement::Reason {
                    using_agent: using_agent.unwrap(),
                    input: input_var.unwrap(),
                    goal: goal.unwrap(),
                    output_type: output_type.unwrap(),
                    bind: bind.unwrap(),
                    span,
                })
            }),
            trivia
        )
    );
    parser.parse_next(input)
}

fn for_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "for",
                paren_delimited((
                    lexeme(simple_identifier_raw),
                    "in",
                    expr,
                )),
                block,
            )
                .with_span()
                .map(|((_, (item_name, _, iterator), body), span)| Statement::ForLoop {
                    item_name,
                    iterator,
                    body,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn if_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "if",
                paren_delimited(expr),
                block,
                opt(preceded(lexeme("else"), else_branch)),
            )
                .with_span()
                .map(|((_, condition, if_body, else_body), span)| Statement::IfCond {
                    condition,
                    if_body,
                    else_body,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn else_branch(input: &mut Input<'_>) -> PResult<ElseBranch> {
    alt((
        if_stmt.map(|stmt| ElseBranch::ElseIf(Box::new(stmt))),
        block.map(ElseBranch::Else),
    ))
    .parse_next(input)
}

fn return_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            ("return", expr)
                .with_span()
                .map(|((_, value), span)| Statement::Return { value, span }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn execute_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            raw_execute_run_parts
                .with_span()
                .map(|((agent_name, kwargs, require_type), span)| Statement::ExecuteRun {
                    agent_name,
                    kwargs,
                    require_type,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn try_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            (
                "try",
                block,
                "catch",
                lexeme('('),
                lexeme(simple_identifier_raw),
                lexeme(':'),
                raw_data_type,
                lexeme(')'),
                block,
            )
                .with_span()
                .map(
                    |((_, try_body, _, _, catch_name, _, catch_type, _, catch_body), span)| {
                        Statement::TryCatch {
                            try_body,
                            catch_name,
                            catch_type,
                            catch_body,
                            span,
                        }
                    },
                ),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn continue_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            "continue"
                .with_span()
                .map(|(_, span)| Statement::Continue(span)),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn break_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            "break"
                .with_span()
                .map(|(_, span)| Statement::Break(span)),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn assert_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            ("assert", expr, opt(preceded(lexeme(','), string_literal)))
                .with_span()
                .map(|((_, condition, message), span)| Statement::Assert {
                    condition,
                    message,
                    span,
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}


fn expression_stmt(input: &mut Input<'_>) -> PResult<Statement> {
    let mut parser = preceded(
        trivia,
        terminated(
            raw_expr
                .with_span()
                .map(|(expression, span)| {
                    Statement::Expression(SpannedExpr {
                        expr: expression,
                        span,
                    })
                }),
            trivia,
        ),
    );

    parser.parse_next(input)
}

fn expr(input: &mut Input<'_>) -> PResult<SpannedExpr> {
    preceded(
        trivia,
        terminated(raw_expr.with_span().map(|(e, span)| SpannedExpr { expr: e, span }), trivia),
    )
    .parse_next(input)
}

fn raw_expr(input: &mut Input<'_>) -> PResult<Expr> {
    let (left, left_span) = alt((
        raw_execute_run_expr,
        raw_array_expr,
        raw_string_expr,
        raw_number_expr,
        raw_bool_expr,
        identifier_or_call_expr,
    ))
    .with_span()
    .parse_next(input)?;

    let left_spanned = SpannedExpr {
        expr: left,
        span: left_span,
    };

    let checkpoint = input.checkpoint();
    let binary_op = if lexeme("==").parse_next(input).is_ok() {
        Some(BinaryOp::Equal)
    } else {
        input.reset(&checkpoint);
        let checkpoint = input.checkpoint();
        if lexeme("!=").parse_next(input).is_ok() {
            Some(BinaryOp::NotEqual)
        } else {
            input.reset(&checkpoint);
            let checkpoint = input.checkpoint();
            if lexeme("<=").parse_next(input).is_ok() {
                Some(BinaryOp::LessEq)
            } else {
                input.reset(&checkpoint);
                let checkpoint = input.checkpoint();
                if lexeme(">=").parse_next(input).is_ok() {
                    Some(BinaryOp::GreaterEq)
                } else {
                    input.reset(&checkpoint);
                    let checkpoint = input.checkpoint();
                    if lexeme("<").parse_next(input).is_ok() {
                        Some(BinaryOp::LessThan)
                    } else {
                        input.reset(&checkpoint);
                        let checkpoint = input.checkpoint();
                        if lexeme(">").parse_next(input).is_ok() {
                            Some(BinaryOp::GreaterThan)
                        } else {
                            input.reset(&checkpoint);
                            None
                        }
                    }
                }
            }
        }
    };

    if let Some(op) = binary_op {
        let right = cut_err(expr).parse_next(input)?;
        Ok(Expr::BinaryOp {
            left: Box::new(left_spanned),
            op,
            right: Box::new(right),
        })
    } else {
        Ok(left_spanned.expr)
    }
}

fn raw_execute_run_expr(input: &mut Input<'_>) -> PResult<Expr> {
    raw_execute_run_parts
        .map(|(agent_name, kwargs, require_type)| Expr::ExecuteRun {
            agent_name,
            kwargs,
            require_type,
        })
        .parse_next(input)
}

fn raw_execute_run_parts(input: &mut Input<'_>) -> PResult<ExecuteRunParts> {
    (
        "execute",
        lexeme(simple_identifier_raw),
        lexeme(".run"),
        paren_delimited(execute_arguments),
    )
        .map(|(_, agent_name, _, (kwargs, require_type))| (agent_name, kwargs, require_type))
        .parse_next(input)
}

fn execute_arguments(input: &mut Input<'_>) -> PResult<(ExecuteKwargs, Option<DataType>)> {
    terminated(separated(0.., execute_argument, lexeme(',')), opt(lexeme(',')))
        .map(|arguments: Vec<ExecuteArgument>| {
            let mut kwargs = Vec::new();
            let mut require_type = None;

            for argument in arguments {
                match argument {
                    ExecuteArgument::Kwarg(name, value) => kwargs.push((name, value)),
                    ExecuteArgument::RequireType(value) => require_type = Some(value),
                }
            }

            (kwargs, require_type)
        })
        .parse_next(input)
}

fn execute_argument(input: &mut Input<'_>) -> PResult<ExecuteArgument> {
    let name = lexeme(simple_identifier_raw).parse_next(input)?;
    lexeme(':').parse_next(input)?;

    if name == "require_type" {
        raw_data_type
            .map(ExecuteArgument::RequireType)
            .parse_next(input)
    } else {
        expr.map(|value| ExecuteArgument::Kwarg(name.clone(), value))
            .parse_next(input)
    }
}

fn raw_array_expr(input: &mut Input<'_>) -> PResult<Expr> {
    bracket_delimited(terminated(
        separated(0.., expr, lexeme(',')),
        opt(lexeme(',')),
    ))
    .map(Expr::ArrayLiteral)
    .parse_next(input)
}

fn raw_string_expr(input: &mut Input<'_>) -> PResult<Expr> {
    raw_string_literal.map(Expr::StringLiteral).parse_next(input)
}

fn raw_number_expr(input: &mut Input<'_>) -> PResult<Expr> {
    raw_number_literal
        .verify(|v: &String| {
            if v.contains('.') {
                v.parse::<f64>()
                    .map_or(false, |parsed| !parsed.is_infinite() && !parsed.is_nan())
            } else {
                v.parse::<i64>().is_ok()
            }
        })
        .map(|v: String| {
            if v.contains('.') {
                Expr::FloatLiteral(v.parse::<f64>().expect("verified"))
            } else {
                Expr::IntLiteral(v.parse::<i64>().expect("verified"))
            }
        })
        .parse_next(input)
}

fn raw_bool_expr(input: &mut Input<'_>) -> PResult<Expr> {
    alt((
        "true".value(Expr::BoolLiteral(true)),
        "false".value(Expr::BoolLiteral(false)),
    ))
    .parse_next(input)
}

fn identifier_or_call_expr(input: &mut Input<'_>) -> PResult<Expr> {
    let (ident_expr, ident_span) = simple_identifier_raw
        .with_span()
        .map(|(name, span)| (Expr::Identifier(name), span))
        .parse_next(input)?;

    let mut current_expr = ident_expr;
    let mut current_span: crate::ast::Span = ident_span;

    let checkpoint = input.checkpoint();
    if let Ok(args) = call_args.parse_next(input) {
        if let Expr::Identifier(name) = current_expr {
            current_expr = Expr::Call(name, args);
        }
    } else {
        input.reset(&checkpoint);
    }

    loop {
        let checkpoint = input.checkpoint();
        if lexeme('.').parse_next(input).is_err() {
            input.reset(&checkpoint);
            break;
        }

        let (segment, seg_span) = simple_identifier_raw.with_span().parse_next(input)?;
        let checkpoint = input.checkpoint();

        if let Ok(args) = call_args.parse_next(input) {
            let base = SpannedExpr {
                expr: current_expr,
                span: current_span.clone(),
            };
            current_expr = Expr::MethodCall(Box::new(base), segment, args);
            current_span = current_span.start..seg_span.end;
            continue;
        }

        input.reset(&checkpoint);

        match current_expr {
            Expr::Identifier(mut name) => {
                name.push('.');
                name.push_str(&segment);
                current_expr = Expr::Identifier(name);
                current_span = current_span.start..seg_span.end;
            }
            _ => {
                let base = SpannedExpr {
                    expr: current_expr,
                    span: current_span.clone(),
                };
                current_expr = Expr::MemberAccess(Box::new(base), segment);
                current_span = current_span.start..seg_span.end;
            }
        }
    }

    Ok(current_expr)
}

fn call_args(input: &mut Input<'_>) -> PResult<Vec<SpannedExpr>> {
    paren_delimited(terminated(separated(0.., expr, lexeme(',')), opt(lexeme(','))))
        .parse_next(input)
}

#[cfg(test)]
fn data_type(input: &mut Input<'_>) -> PResult<DataType> {
    preceded(trivia, terminated(raw_data_type, trivia)).parse_next(input)
}

fn raw_data_type(input: &mut Input<'_>) -> PResult<DataType> {
    alt((
        "string".span().map(DataType::String),
        "int".span().map(DataType::Int),
        "float".span().map(DataType::Float),
        "boolean".span().map(DataType::Boolean),
        (
            "list",
            lexeme('<'),
            cut_err(raw_data_type),
            cut_err(lexeme('>')),
        )
            .with_span()
            .map(|((_, _, inner, _), span)| DataType::List(Box::new(inner), span)),
        scoped_identifier_raw
            .with_span()
            .map(|(name, span)| DataType::Custom(name, span)),
    ))
    .parse_next(input)
}

#[cfg(test)]
fn identifier(input: &mut Input<'_>) -> PResult<String> {
    preceded(trivia, terminated(simple_identifier_raw, trivia)).parse_next(input)
}

fn string_literal(input: &mut Input<'_>) -> PResult<String> {
    preceded(trivia, terminated(raw_string_literal, trivia)).parse_next(input)
}

fn raw_string_literal(input: &mut Input<'_>) -> PResult<String> {
    delimited('"', cut_err(take_till(0.., '"')), cut_err('"'))
        .map(|value: &str| value.to_owned())
        .parse_next(input)
}

fn integer_literal_u32(input: &mut Input<'_>) -> PResult<u32> {
    preceded(trivia, terminated(raw_number_literal, trivia))
        .try_map(|value| value.parse::<u32>())
        .parse_next(input)
}

fn raw_number_literal(input: &mut Input<'_>) -> PResult<String> {
    (opt('-'), digit1, opt(('.', digit1)))
        .take()
        .map(|value: &str| value.to_owned())
        .parse_next(input)
}

fn scoped_identifier(input: &mut Input<'_>) -> PResult<String> {
    preceded(trivia, terminated(scoped_identifier_raw, trivia)).parse_next(input)
}

fn scoped_identifier_raw(input: &mut Input<'_>) -> PResult<String> {
    let mut identifier = simple_identifier_raw.parse_next(input)?;

    loop {
        let checkpoint = input.checkpoint();
        if lexeme('.').parse_next(input).is_err() {
            input.reset(&checkpoint);
            break;
        }

        identifier.push('.');
        identifier.push_str(&simple_identifier_raw.parse_next(input)?);
    }

    Ok(identifier)
}

fn simple_identifier_raw(input: &mut Input<'_>) -> PResult<String> {
    (
        take_while(1..=1, |c: char| c.is_ascii_alphabetic()),
        take_while(0.., |c: char| c.is_ascii_alphanumeric() || c == '_'),
    )
        .take()
        .map(|value: &str| value.to_owned())
        .parse_next(input)
}

fn lexeme<'a, O, P>(parser: P) -> impl Parser<Input<'a>, O, ErrMode<ContextError>>
where
    P: Parser<Input<'a>, O, ErrMode<ContextError>>,
{
    preceded(trivia, terminated(parser, trivia))
}

fn paren_delimited<'a, O, P>(parser: P) -> impl Parser<Input<'a>, O, ErrMode<ContextError>>
where
    P: Parser<Input<'a>, O, ErrMode<ContextError>>,
{
    delimited(lexeme('('), parser, cut_err(lexeme(')')))
}

fn bracket_delimited<'a, O, P>(parser: P) -> impl Parser<Input<'a>, O, ErrMode<ContextError>>
where
    P: Parser<Input<'a>, O, ErrMode<ContextError>>,
{
    delimited(lexeme('['), parser, cut_err(lexeme(']')))
}

fn brace_delimited<'a, O, P>(parser: P) -> impl Parser<Input<'a>, O, ErrMode<ContextError>>
where
    P: Parser<Input<'a>, O, ErrMode<ContextError>>,
{
    delimited(lexeme('{'), parser, cut_err(lexeme('}')))
}

fn comma_separated0<'a, O, P>(parser: P) -> impl Parser<Input<'a>, Vec<O>, ErrMode<ContextError>>
where
    P: Parser<Input<'a>, O, ErrMode<ContextError>>,
{
    terminated(separated(0.., parser, lexeme(',')), opt(lexeme(',')))
}

fn trivia(input: &mut Input<'_>) -> PResult<()> {
    loop {
        let checkpoint = input.checkpoint();
        if whitespace(input).is_ok() || comment(input).is_ok() {
            continue;
        }

        input.reset(&checkpoint);
        break;
    }

    Ok(())
}

fn whitespace(input: &mut Input<'_>) -> PResult<()> {
    take_while(1.., |c: char| c.is_whitespace())
        .void()
        .parse_next(input)
}

fn comment(input: &mut Input<'_>) -> PResult<()> {
    ("//", take_till(0.., '\n'), opt('\n')).void().parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BinaryOp, DataType, Expr, SettingValue, Statement};

    #[test]
    fn parses_identifier() {
        assert_eq!(parse_identifier("Researcher").unwrap(), "Researcher");
    }

    #[test]
    fn rejects_invalid_identifier_with_precise_span() {
        let error = parse_identifier("1Researcher").unwrap_err();
        assert_eq!(error.span(), Some(&(0..1)));
    }

    #[test]
    fn parses_string_literal() {
        assert_eq!(parse_string_literal("\"hello world\"").unwrap(), "hello world");
    }

    #[test]
    fn parses_primitive_and_list_data_types() {
        assert_eq!(parse_data_type("string").unwrap(), DataType::String(0..6));
        assert_eq!(parse_data_type("int").unwrap(), DataType::Int(0..3));
        assert_eq!(parse_data_type("float").unwrap(), DataType::Float(0..5));
        assert_eq!(parse_data_type("boolean").unwrap(), DataType::Boolean(0..7));
        assert_eq!(
            parse_data_type("list<string>").unwrap(),
            DataType::List(Box::new(DataType::String(5..11)), 0..12)
        );
    }

    #[test]
    fn parses_type_declaration_with_constraints() {
        let source = r#"
            type SearchResult {
                url: string
                confidence: float @min(0)
            }
        "#;

        let declaration = parse_type_decl(source).unwrap();

        assert_eq!(declaration.name, "SearchResult");
        assert_eq!(declaration.fields.len(), 2);
        assert_eq!(declaration.fields[0].name, "url");
        assert_eq!(declaration.fields[1].constraints.len(), 1);
    }

    #[test]
    fn reports_eof_span_for_malformed_type_declaration() {
        let source = "type SearchResult { url: string";
        let error = parse_type_decl(source).unwrap_err();
        assert_eq!(error.span(), Some(&(31..31)));
    }

    #[test]
    fn parses_tool_declaration_with_invoke_path() {
        let source = r#"
            tool AnalyzeSentiment(text: string) -> float {
                invoke: module("scripts.analysis").function("get_sentiment")
            }
        "#;

        let declaration = parse_tool_decl(source).unwrap();

        assert_eq!(declaration.name, "AnalyzeSentiment");
        assert_eq!(declaration.arguments.len(), 1);
        assert_eq!(declaration.return_type, Some(DataType::Float(52..57)));
        assert_eq!(
            declaration.invoke_path.as_deref(),
            Some(r#"module("scripts.analysis").function("get_sentiment")"#)
        );
    }

    #[test]
    fn reports_eof_span_for_malformed_tool_declaration() {
        let source = "tool Search(query: string -> string";
        let error = parse_tool_decl(source).unwrap_err();
        assert_eq!(error.span(), Some(&(26..27)));
    }

    #[test]
    fn parses_agent_declaration() {
        let source = r#"
            agent Researcher extends BaseResearcher {
                client = FastOpenAI
                system_prompt = "Stay deterministic."
                tools = [WebScraper, FileSystem.write]
                settings = {
                    max_steps: 5,
                    temperature: 0.1,
                    allow_fallback: false
                }
            }
        "#;

        let declaration = parse_agent_decl(source).unwrap();

        assert_eq!(declaration.name, "Researcher");
        assert_eq!(declaration.extends.as_deref(), Some("BaseResearcher"));
        assert_eq!(declaration.client.as_deref(), Some("FastOpenAI"));
        assert_eq!(
            declaration.tools,
            vec!["WebScraper".to_owned(), "FileSystem.write".to_owned()]
        );
        assert_eq!(declaration.settings.entries.len(), 3);
        assert_eq!(
            declaration.settings.entries[1].value,
            SettingValue::Float(0.1)
        );
    }

    #[test]
    fn reports_eof_span_for_malformed_agent_declaration() {
        let source = "agent Researcher { client = FastOpenAI";
        let error = parse_agent_decl(source).unwrap_err();
        assert_eq!(error.span(), Some(&(38..38)));
    }

    #[test]
    fn parses_workflow_declaration_with_execute_expression() {
        let source = r#"
            workflow Analyze(company: string) -> string {
                let report = execute Researcher.run(
                    task: company,
                    require_type: string,
                )
                if (report == "ok") {
                    return report
                }
                return "fallback"
            }
        "#;

        let declaration = parse_workflow_decl(source).unwrap();

        assert_eq!(declaration.name, "Analyze");
        assert_eq!(declaration.arguments.len(), 1);
        assert_eq!(declaration.body.statements.len(), 3);

        match &declaration.body.statements[0] {
            Statement::LetDecl { value, .. } => match &value.expr {
                Expr::ExecuteRun {
                    agent_name,
                    require_type,
                    ..
                } => {
                    assert_eq!(agent_name, "Researcher");
                    assert_eq!(require_type, &Some(DataType::String(181..187)));
                }
                other => panic!("expected execute expression, found {other:?}"),
            },
            other => panic!("expected let declaration, found {other:?}"),
        }

        match &declaration.body.statements[1] {
            Statement::IfCond { condition, .. } => match &condition.expr {
                Expr::BinaryOp { op, .. } => assert_eq!(op, &BinaryOp::Equal),
                other => panic!("expected equality condition, found {other:?}"),
            },
            other => panic!("expected if statement, found {other:?}"),
        }
    }

    #[test]
    fn reports_eof_span_for_malformed_workflow_declaration() {
        let source = "workflow Analyze() -> string { return \"ok\"";
        let error = parse_workflow_decl(source).unwrap_err();
        assert_eq!(error.span(), Some(&(42..42)));
    }

    #[test]
    fn parses_document_root_and_matches_snapshot() {
        let source = r#"
            import { WebScraper } from "@claw/tools.browser"

            client FastOpenAI {
                provider = "openai"
                model = "gpt-4o-mini"
                retries = 3
                endpoint = env("OPENAI_BASE_URL")
                api_key = env("OPENAI_API_KEY")
            }

            type SearchResult {
                url: string
                confidence: float @min(0)
            }

            tool AnalyzeSentiment(text: string) -> float {
                invoke: module("scripts.analysis").function("get_sentiment")
            }

            agent Researcher {
                client = FastOpenAI
                system_prompt = "Stay deterministic."
                tools = [WebScraper, AnalyzeSentiment]
                settings = {
                    max_steps: 5,
                    temperature: 0.1
                }
            }

            workflow Analyze(company: string) -> string {
                let report = execute Researcher.run(
                    task: company,
                    require_type: string,
                )
                return report
            }

            listener OnSlackMessage(event: Events.Slack.Message) {
                event.reply("done")
            }

            test "smoke" {
                return "ok"
            }

            mock Researcher {
                output: "mocked_output"
            }
        "#;

        let document = parse_document(source).unwrap();

        insta::assert_debug_snapshot!(document);
    }

    #[test]
    fn rejects_overflow_integer_literal_without_panic() {
        // Per specs/12-Security-Model.md §7.2: integers exceeding i64::MAX must produce
        // CompilerError::ParseError, not panic via .expect()
        let source = "type Overflow { value: int @min(99999999999999999999999) }";
        let result = parse_document(source);
        assert!(result.is_err(), "overflow integer must produce an error, not panic");
    }

    #[test]
    fn rejects_overflow_float_literal_without_panic() {
        // Per specs/12-Security-Model.md §7.2: floats resolving to infinity must produce
        // CompilerError::ParseError, not panic
        let source = r#"type Overflow { value: float @min(9999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999.0) }"#;
        let result = parse_document(source);
        assert!(result.is_err(), "overflow float must produce an error, not panic");
    }

    #[test]
    fn test_parse_tool_with_using_fetch() {
        let src = r#"
tool WebSearch(query: string) -> SearchResult {
    using: fetch
}
"#;
        let doc = super::parse(src).unwrap();
        assert_eq!(doc.tools[0].using, Some(super::UsingExpr::Fetch));
        assert!(doc.tools[0].invoke_path.is_none());
    }

    #[test]
    fn test_parse_tool_with_using_mcp() {
        let src = r#"
tool BraveSearch(query: string) -> SearchResult {
    using: mcp("brave-search")
}
"#;
        let doc = super::parse(src).unwrap();
        assert_eq!(doc.tools[0].using, Some(super::UsingExpr::Mcp("brave-search".to_string())));
    }

    #[test]
    fn test_parse_tool_with_test_block() {
        let src = r#"
tool WebSearch(query: string) -> SearchResult {
    using: fetch
    test {
        input:  { query: "rust language" }
        expect: { url: !empty }
    }
}
"#;
        let doc = super::parse(src).unwrap();
        assert!(doc.tools[0].test_block.is_some());
        let tb = doc.tools[0].test_block.as_ref().unwrap();
        assert_eq!(tb.expect[0].1, super::ExpectOp::NotEmpty);
    }

    #[test]
    fn test_parse_synthesizer_decl() {
        let src = r#"
client MyClaude {
    provider = "anthropic"
    model = "claude-sonnet-4-6"
}
synthesizer DefaultSynth {
    client = MyClaude
    temperature = 0.1
}
"#;
        let doc = super::parse(src).unwrap();
        assert_eq!(doc.synthesizers[0].name, "DefaultSynth");
        assert_eq!(doc.synthesizers[0].client, "MyClaude");
    }

    #[test]
    fn test_parse_reason_stmt() {
        let src = r#"
workflow ResearchAndDecide(query: string) -> Decision {
    let raw: SearchResult = execute Searcher.run(query: query)
    reason {
        using:       Writer
        input:       raw
        goal:        "Analyze the results"
        output_type: Decision
        bind:        decision
    }
    return decision
}
"#;
        let doc = super::parse(src).unwrap();
        let stmts = &doc.workflows[0].body.statements;
        assert!(stmts.iter().any(|s| matches!(s, super::Statement::Reason { .. })));
    }

    #[test]
    fn test_parse_error_tool_with_both_invoke_and_using() {
        let src = r#"
tool WebSearch(query: string) -> SearchResult {
    invoke: module("scripts/search").function("run")
    using: fetch
}
"#;
        assert!(super::parse(src).is_err());
    }
}
