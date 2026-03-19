use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use clawc::ast::{Document, TestDecl};
use clawc::codegen;
use clawc::config::{BuildLanguage, OpenClawConfig};
use clawc::errors::CompilerError;
use clawc::{parser, semantic};
use notify::{recommended_watcher, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Parser)]
#[command(author, version, about = "Claw language CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Init(InitArgs),
    Build(BuildArgs),
    Dev(DevArgs),
    Test(TestArgs),
}

#[derive(Debug, clap::Args)]
struct InitArgs {
    #[arg(long, default_value = "claw.json")]
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
    #[arg(long, default_value = "claw.json")]
    config: PathBuf,
}

#[derive(Debug, clap::Args)]
struct DevArgs {
    #[arg(long, default_value = "claw.json")]
    config: PathBuf,
    #[arg(long, default_value = "8080")]
    port: u16,
}

#[derive(Debug, clap::Args)]
struct TestArgs {
    source: Option<PathBuf>,
    #[arg(long)]
    filter: Option<String>,
    #[arg(long, default_value = "claw.json")]
    config: PathBuf,
}

#[derive(Debug, Clone)]
struct BuildRequest {
    source: PathBuf,
    language: BuildLanguage,
    output_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GatewayCommand {
    program: PathBuf,
    args: Vec<String>,
    project_root: PathBuf,
}

#[derive(Debug, Error)]
enum OpenClawCliError {
    #[error("{rendered}")]
    Compiler {
        error: CompilerError,
        rendered: String,
    },
    #[error("{0}")]
    Codegen(String),
    #[error("{0}")]
    Message(String),
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("{error}");
        std::process::exit(exit_code_for_error(&error));
    }
}

fn exit_code_for_error(error: &OpenClawCliError) -> i32 {
    match error {
        OpenClawCliError::Compiler { error, .. } => match error {
            CompilerError::ParseError { .. } => 1,
            CompilerError::Io { .. } => 4,
            CompilerError::UndefinedTool { .. }
            | CompilerError::UndefinedAgent { .. }
            | CompilerError::UndefinedClient { .. }
            | CompilerError::UndefinedType { .. }
            | CompilerError::TypeMismatch { .. }
            | CompilerError::CircularType { .. }
            | CompilerError::MissingReturn { .. }
            | CompilerError::InvalidControlFlow { .. }
            | CompilerError::InvalidAssertOutsideTest { .. }
            | CompilerError::DuplicateSymbol { .. }
            | CompilerError::UnsupportedConstraint { .. }
            | CompilerError::InvalidConstraintValue { .. }
            | CompilerError::BamlSignatureConflict { .. }
            | CompilerError::CircularAgentExtends { .. } => 2,
        },
        OpenClawCliError::Codegen(_) => 3,
        OpenClawCliError::Message(_) => 1,
    }
}

fn run(cli: Cli) -> Result<(), OpenClawCliError> {
    match cli.command {
        Commands::Init(args) => run_init(args),
        Commands::Build(args) => run_build_command(args),
        Commands::Dev(args) => run_dev(args),
        Commands::Test(args) => run_test(args),
    }
}

fn run_init(args: InitArgs) -> Result<(), OpenClawCliError> {
    // ── Step 1: Node.js version check ──────────────────────────────────
    ensure_node_version()?;

    // ── Step 2: Create claw.json ──────────────────────────────────────
    if args.path.exists() && !args.force {
        println!("[init] {} already exists (use --force to overwrite)", args.path.display());
    } else {
        let default_source = if Path::new("example.claw").exists() {
            PathBuf::from("example.claw")
        } else {
            PathBuf::from("src/pipeline.claw")
        };

        let config = OpenClawConfig::template(default_source);
        config.write_pretty(&args.path).map_err(|error| {
            let rendered = error.to_string();
            compiler_error(error, rendered)
        })?;
        println!("[init] created {}", args.path.display());
    }

    // ── Step 3: Create .env from .env.example if missing ───────────────
    ensure_env_file()?;

    // ── Step 4: Install npm dependencies ───────────────────────────────
    ensure_npm_installed()?;

    // ── Step 5: Create .claw/ state directory ──────────────────────
    ensure_state_dir()?;

    // ── Step 6: Initial build ──────────────────────────────────────────
    let config = OpenClawConfig::load(&args.path).map_err(|error| {
        let rendered = error.to_string();
        compiler_error(error, rendered)
    })?;
    let request = resolve_build_request(
        &BuildArgs {
            source: None,
            lang: None,
            watch: false,
            config: args.path.clone(),
        },
        Some(&config),
    )?;
    match run_build_once(&request) {
        Ok(path) => println!("[init] built {}", path.display()),
        Err(error) => eprintln!("[init] build warning: {error}"),
    }

    println!("[init] ready — run `claw dev` to start");
    Ok(())
}

fn run_dev(args: DevArgs) -> Result<(), OpenClawCliError> {
    // Auto-bootstrap: config, node, npm deps, state dir
    if !args.config.exists() {
        println!("[dev] no {} found — running init first", args.config.display());
        run_init(InitArgs {
            path: args.config.clone(),
            force: false,
        })?;
    } else {
        ensure_bootstrapped()?;
    }

    let config = OpenClawConfig::load(&args.config)
        .map_err(|error| {
            let rendered = error.to_string();
            compiler_error(error, rendered)
        })?;

    // Initial build before starting the gateway
    let build_args = BuildArgs {
        source: None,
        lang: None,
        watch: false,
        config: args.config.clone(),
    };
    let request = resolve_build_request(&build_args, Some(&config))?;
    let initial_output = run_build_once(&request)?;
    println!("[dev] built {}", initial_output.display());

    println!("[dev] starting gateway on port {}", args.port);
    let mut gateway_child = start_gateway(args.port, &config, &args.config)?;

    // Set up file watcher for hot-reload
    let (sender, receiver) = mpsc::channel();
    let mut watcher = recommended_watcher(move |result| {
        let _ = sender.send(result);
    })
    .map_err(|error| OpenClawCliError::Message(format!("failed to start file watcher: {error}")))?;

    let mut watched_source = request.source.clone();
    watch_path(&mut watcher, &watched_source)?;
    if args.config.exists() {
        watch_path(&mut watcher, &args.config)?;
    }

    println!(
        "[dev] watching {} for changes (ctrl+c to stop)",
        watched_source.display()
    );

    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = shutdown_tx.send(());
    })
    .map_err(|error| OpenClawCliError::Message(format!("failed to set ctrl+c handler: {error}")))?;

    loop {
        if shutdown_rx.try_recv().is_ok() {
            println!("[dev] shutting down...");
            shutdown_gateway(&config, args.port, &mut gateway_child)?;
            return Ok(());
        }

        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(_event)) => {
                let _ = drain_watch_burst(&receiver);
                let fresh_config = if args.config.exists() {
                    Some(OpenClawConfig::load(&args.config).map_err(|error| {
                        let rendered = error.to_string();
                        compiler_error(error, rendered)
                    })?)
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
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                let _ = shutdown_gateway(&config, args.port, &mut gateway_child);
                return Err(OpenClawCliError::Message(
                    "watch channel closed unexpectedly".to_owned(),
                ));
            }
        }
    }
}

fn start_gateway(
    port: u16,
    config: &OpenClawConfig,
    config_path: &Path,
) -> Result<Child, OpenClawCliError> {
    let gateway = resolve_gateway_command(config_path, config)?;
    let mut command = Command::new(&gateway.program);
    command.args(&gateway.args);
    command.current_dir(&gateway.project_root);
    command.env("CLAW_GATEWAY_PORT", port.to_string());
    command.env("CLAW_GATEWAY_API_KEY_ENV", &config.gateway.api_key_env);
    command.env("CLAW_PROJECT_ROOT", &gateway.project_root);
    command.env("CLAW_SANDBOX_BACKEND", &config.runtimes.sandbox_backend);
    command.env("CLAW_PYTHON_SANDBOX_IMAGE", &config.runtimes.python_image);
    command.env("CLAW_NODE_SANDBOX_IMAGE", &config.runtimes.node_image);
    if let Some(cors_origin) = &config.gateway.cors_origin {
        command.env("CLAW_GATEWAY_CORS_ORIGIN", cors_origin);
    }

    command.spawn().map_err(|error| {
        OpenClawCliError::Message(format!(
            "failed to start gateway via {}: {error}",
            gateway.program.display()
        ))
    })
}

fn resolve_gateway_command(
    config_path: &Path,
    config: &OpenClawConfig,
) -> Result<GatewayCommand, OpenClawCliError> {
    let project_root = resolve_project_root(config_path)?;

    if let Some(executable) = &config.gateway.executable {
        let program = resolve_explicit_gateway_executable(&project_root, executable);
        return Ok(GatewayCommand {
            program,
            args: Vec::new(),
            project_root,
        });
    }

    let local_bin = project_root
        .join("node_modules")
        .join(".bin")
        .join(gateway_binary_name());
    if local_bin.exists() {
        return Ok(GatewayCommand {
            program: local_bin,
            args: Vec::new(),
            project_root,
        });
    }

    if let Some(program) = find_command_in_path(gateway_binary_name()) {
        return Ok(GatewayCommand {
            program,
            args: Vec::new(),
            project_root,
        });
    }

    let gateway_entry = project_root.join("claw-gateway").join("src").join("server.ts");
    if gateway_entry.exists() {
        return Ok(GatewayCommand {
            program: PathBuf::from("node"),
            args: vec![
                "--experimental-strip-types".to_owned(),
                gateway_entry.to_string_lossy().into_owned(),
            ],
            project_root,
        });
    }

    Err(OpenClawCliError::Message(
        "Gateway binary not found. Set 'gateway.executable' in claw.json, or ensure openclaw-gateway is in this workspace."
            .to_owned(),
    ))
}

fn resolve_project_root(config_path: &Path) -> Result<PathBuf, OpenClawCliError> {
    let absolute_config = if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                OpenClawCliError::Message(format!(
                    "failed to resolve current directory: {error}"
                ))
            })?
            .join(config_path)
    };

    let project_root = absolute_config
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    Ok(fs::canonicalize(&project_root).unwrap_or(project_root))
}

fn resolve_explicit_gateway_executable(project_root: &Path, executable: &Path) -> PathBuf {
    if executable.is_absolute() {
        executable.to_path_buf()
    } else {
        let project_relative = project_root.join(executable);
        if executable.components().count() > 1 || project_relative.exists() {
            project_relative
        } else {
            executable.to_path_buf()
        }
    }
}

fn gateway_binary_name() -> &'static str {
    if cfg!(windows) {
        "claw-gateway.cmd"
    } else {
        "claw-gateway"
    }
}

fn find_command_in_path(command_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path_var) {
        let candidate = directory.join(command_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn shutdown_gateway(
    config: &OpenClawConfig,
    port: u16,
    child: &mut Child,
) -> Result<(), OpenClawCliError> {
    if child
        .try_wait()
        .map_err(|error| {
            OpenClawCliError::Message(format!("failed to inspect gateway process: {error}"))
        })?
        .is_some()
    {
        return Ok(());
    }

    let api_key = std::env::var(&config.gateway.api_key_env).ok();
    let _ = request_gateway_shutdown(port, api_key.as_deref());

    if wait_for_child_exit(child, Duration::from_secs(5))? {
        return Ok(());
    }

    child
        .kill()
        .map_err(|error| {
            OpenClawCliError::Message(format!("failed to terminate gateway process: {error}"))
        })?;
    let _ = child.wait();
    Ok(())
}

fn request_gateway_shutdown(
    port: u16,
    api_key: Option<&str>,
) -> Result<(), OpenClawCliError> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))
        .map_err(|error| {
            OpenClawCliError::Message(format!(
                "failed to connect to gateway shutdown endpoint: {error}"
            ))
        })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| {
            OpenClawCliError::Message(format!(
                "failed to set gateway shutdown read timeout: {error}"
            ))
        })?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| {
            OpenClawCliError::Message(format!(
                "failed to set gateway shutdown write timeout: {error}"
            ))
        })?;

    let request = build_shutdown_request(port, api_key);
    stream
        .write_all(request.as_bytes())
        .map_err(|error| {
            OpenClawCliError::Message(format!(
                "failed to send gateway shutdown request: {error}"
            ))
        })?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| {
            OpenClawCliError::Message(format!(
                "failed to read gateway shutdown response: {error}"
            ))
        })?;

    if response.starts_with("HTTP/1.1 200")
        || response.starts_with("HTTP/1.1 202")
        || response.starts_with("HTTP/1.0 200")
        || response.starts_with("HTTP/1.0 202")
    {
        return Ok(());
    }

    Err(OpenClawCliError::Message(format!(
        "gateway shutdown endpoint returned an unexpected response: {}",
        response.lines().next().unwrap_or("<empty>")
    )))
}

fn build_shutdown_request(port: u16, api_key: Option<&str>) -> String {
    let mut request = format!(
        "POST /shutdown HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: 0\r\n"
    );
    if let Some(api_key) = api_key {
        request.push_str(&format!("x-claw-key: {api_key}\r\n"));
    }
    request.push_str("\r\n");
    request
}

fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<bool, OpenClawCliError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if child
            .try_wait()
            .map_err(|error| {
                OpenClawCliError::Message(format!(
                    "failed while waiting for gateway process: {error}"
                ))
            })?
            .is_some()
        {
            return Ok(true);
        }

        if std::time::Instant::now() >= deadline {
            return Ok(false);
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

fn drain_watch_burst(
    receiver: &mpsc::Receiver<Result<Event, notify::Error>>,
) -> Result<(), OpenClawCliError> {
    let deadline = std::time::Instant::now() + Duration::from_millis(100);
    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            return Ok(());
        }

        match receiver.recv_timeout(deadline - now) {
            Ok(Ok(_)) => continue,
            Ok(Err(error)) => {
                return Err(OpenClawCliError::Message(format!("watch error: {error}")));
            }
            Err(RecvTimeoutError::Timeout) => return Ok(()),
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn run_build_command(args: BuildArgs) -> Result<(), OpenClawCliError> {
    ensure_bootstrapped()?;

    let config = if args.source.is_none() {
        Some(
            OpenClawConfig::load(&args.config)
                .map_err(|error| {
                    let rendered = error.to_string();
                    compiler_error(error, rendered)
                })?,
        )
    } else {
        None
    };

    let request = resolve_build_request(&args, config.as_ref())?;
    if args.watch {
        return run_watch_mode(args, request);
    }

    run_build_once(&request).map(|_| ())
}

fn run_test(args: TestArgs) -> Result<(), OpenClawCliError> {
    ensure_bootstrapped()?;

    let config = if args.source.is_none() {
        Some(
            OpenClawConfig::load(&args.config)
                .map_err(|error| {
                    let rendered = error.to_string();
                    compiler_error(error, rendered)
                })?,
        )
    } else {
        None
    };

    let source_path = resolve_test_source(&args, config.as_ref())?;
    let source = read_source_file(&source_path).map_err(|error| {
        let rendered = error.to_string();
        compiler_error(error, rendered)
    })?;
    let document = compile_document(&source).map_err(|error| {
        let rendered = format_compiler_error(&source_path, &source, &error);
        compiler_error(error, rendered)
    })?;

    let selected_tests = select_tests(&document, args.filter.as_deref());
    if selected_tests.is_empty() {
        println!("No tests matched filter.");
        return Ok(());
    }
    let selected_test_count = selected_tests.len();

    println!(
        "Running {} tests from {}...",
        selected_test_count,
        source_path.display()
    );
    println!();

    let runner = resolve_test_runner_command(&args.config)?;
    let manifest = TestManifest {
        compiled: CompiledDocument {
            ast_hash: codegen::document_ast_hash(&document),
            document: &document,
        },
        tests: selected_tests,
    };
    let manifest_json = serde_json::to_vec(&manifest).map_err(|error| {
        OpenClawCliError::Message(format!("failed to serialize test manifest: {error}"))
    })?;

    let mut child = Command::new(&runner.program)
        .args(&runner.args)
        .current_dir(&runner.project_root)
        .env("CLAW_PROJECT_ROOT", &runner.project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            OpenClawCliError::Message(format!(
                "failed to start test runner via {}: {error}",
                runner.program.display()
            ))
        })?;

    {
        let Some(stdin) = child.stdin.as_mut() else {
            return Err(OpenClawCliError::Message(
                "failed to open test runner stdin".to_owned(),
            ));
        };
        stdin
            .write_all(&manifest_json)
            .map_err(|error| OpenClawCliError::Message(format!("failed to write test manifest: {error}")))?;
    }
    child.stdin.take();

    let stdout = child.stdout.take().ok_or_else(|| {
        OpenClawCliError::Message("failed to open test runner stdout".to_owned())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        OpenClawCliError::Message("failed to open test runner stderr".to_owned())
    })?;

    let mut summary: Option<TestSummaryLine> = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|error| {
            OpenClawCliError::Message(format!("failed to read test runner output: {error}"))
        })?;
        if line.trim().is_empty() {
            continue;
        }

        let parsed: TestRunnerLine = serde_json::from_str(&line).map_err(|error| {
            OpenClawCliError::Message(format!("failed to parse test runner output: {error}"))
        })?;

        if parsed.summary.unwrap_or(false) {
            summary = Some(TestSummaryLine {
                passed: parsed.passed.unwrap_or(0),
                failed: parsed.failed.unwrap_or(0),
                total_ms: parsed.total_ms.unwrap_or(0),
            });
            continue;
        }

        let Some(name) = parsed.name else {
            continue;
        };
        let Some(status) = parsed.status else {
            continue;
        };
        let duration_ms = parsed.duration_ms.unwrap_or(0);
        if status == "pass" {
            println!("  PASS  {name} ({duration_ms}ms)");
        } else {
            println!("  FAIL  {name} ({duration_ms}ms)");
            if let Some(error) = parsed.error {
                println!("        {error}");
            }
            if let Some(node_path) = parsed.node_path {
                println!("        at {node_path}");
            }
        }
    }

    let stderr_output = {
        let mut buffer = String::new();
        BufReader::new(stderr)
            .read_to_string(&mut buffer)
            .map_err(|error| {
                OpenClawCliError::Message(format!("failed to read test runner stderr: {error}"))
            })?;
        buffer
    };

    let status = child.wait().map_err(|error| {
        OpenClawCliError::Message(format!("failed to wait for test runner: {error}"))
    })?;

    let summary = summary.unwrap_or_else(|| TestSummaryLine {
        passed: selected_test_count,
        failed: 0,
        total_ms: 0,
    });

    println!();
    println!(
        "Results: {} passed, {} failed ({}ms total)",
        summary.passed, summary.failed, summary.total_ms
    );

    if !status.success() || summary.failed > 0 {
        let message = if !stderr_output.trim().is_empty() {
            stderr_output.trim().to_owned()
        } else {
            format!("{} tests failed", summary.failed.max(1))
        };
        return Err(OpenClawCliError::Message(message));
    }

    Ok(())
}

fn resolve_build_request(
    args: &BuildArgs,
    config: Option<&OpenClawConfig>,
) -> Result<BuildRequest, OpenClawCliError> {
    let source = args
        .source
        .clone()
        .or_else(|| config.map(|config| config.build.source.clone()))
        .ok_or_else(|| {
            OpenClawCliError::Message(
                "no .claw source provided and claw.json was not loaded".to_owned(),
            )
        })?;

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

fn resolve_test_source(
    args: &TestArgs,
    config: Option<&OpenClawConfig>,
) -> Result<PathBuf, OpenClawCliError> {
    args.source
        .clone()
        .or_else(|| config.map(|config| config.build.source.clone()))
        .ok_or_else(|| {
            OpenClawCliError::Message(
                "no .claw source provided and claw.json was not loaded".to_owned(),
            )
        })
}

fn run_watch_mode(args: BuildArgs, initial_request: BuildRequest) -> Result<(), OpenClawCliError> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = recommended_watcher(move |result| {
        let _ = sender.send(result);
    })
    .map_err(|error| OpenClawCliError::Message(format!("failed to start file watcher: {error}")))?;

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
                let _ = drain_watch_burst(&receiver);
                let config = if args.source.is_none() && args.config.exists() {
                    Some(
                        OpenClawConfig::load(&args.config)
                            .map_err(|error| {
                                let rendered = error.to_string();
                                compiler_error(error, rendered)
                            })?,
                    )
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
            Err(error) => {
                return Err(OpenClawCliError::Message(format!(
                    "watch channel closed: {error}"
                )));
            }
        }
    }
}

fn watch_path(
    watcher: &mut RecommendedWatcher,
    path: &Path,
) -> Result<(), OpenClawCliError> {
    watcher
        .watch(path, RecursiveMode::NonRecursive)
        .map_err(|error| {
            OpenClawCliError::Message(format!(
                "failed to watch {}: {error}",
                path.display()
            ))
        })
}

fn resolve_test_runner_command(config_path: &Path) -> Result<GatewayCommand, OpenClawCliError> {
    let project_root = resolve_project_root(config_path)?;
    let test_runner = project_root
        .join("claw-gateway")
        .join("src")
        .join("engine")
        .join("test-runner.ts");

    if !test_runner.exists() {
        return Err(OpenClawCliError::Message(format!(
            "Gateway test runner not found at {}",
            test_runner.display()
        )));
    }

    Ok(GatewayCommand {
        program: PathBuf::from("node"),
        args: vec![
            "--experimental-strip-types".to_owned(),
            test_runner.to_string_lossy().into_owned(),
        ],
        project_root,
    })
}

fn select_tests<'a>(document: &'a Document, filter: Option<&str>) -> Vec<&'a TestDecl> {
    let Some(filter) = filter.map(|value| value.to_ascii_lowercase()) else {
        return document.tests.iter().collect();
    };

    document
        .tests
        .iter()
        .filter(|test| test.name.to_ascii_lowercase().contains(&filter))
        .collect()
}

fn run_build_once(request: &BuildRequest) -> Result<PathBuf, OpenClawCliError> {
    let source = read_source_file(&request.source).map_err(|error| {
        let rendered = error.to_string();
        compiler_error(error, rendered)
    })?;
    let document = compile_document(&source).map_err(|error| {
        let rendered = format_compiler_error(&request.source, &source, &error);
        compiler_error(error, rendered)
    })?;
    let generated = generate_sdk(&document, request.language)?;
    let output_root = std::env::current_dir().map_err(|source| CompilerError::Io {
        path: PathBuf::from("."),
        source,
    });
    let output_root = output_root.map_err(|error| {
        let rendered = error.to_string();
        compiler_error(error, rendered)
    })?;

    write_generated_artifacts(
        &output_root,
        &request.output_dir,
        request.language,
        &document,
        &generated,
    )
    .map_err(|error| {
        let rendered = error.to_string();
        compiler_error(error, rendered)
    })
}

fn compile_document(source: &str) -> Result<Document, CompilerError> {
    let document = parser::parse(source)?;
    semantic::analyze(&document)?;
    Ok(document)
}

fn generate_sdk(document: &Document, language: BuildLanguage) -> Result<String, OpenClawCliError> {
    match language {
        BuildLanguage::Ts => codegen::generate_ts(document),
        BuildLanguage::Python => codegen::generate_python(document),
    }
    .map_err(|error| {
        let rendered = error.to_string();
        compiler_error(error, rendered)
    })
}

fn compiler_error(error: CompilerError, rendered: String) -> OpenClawCliError {
    OpenClawCliError::Compiler { error, rendered }
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
    write_baml_artifacts(output_root, document)?;
    Ok(output_path)
}

fn write_baml_artifacts(output_root: &Path, document: &Document) -> Result<(), CompilerError> {
    let resolved = codegen::resolve_agents(document)?;
    let call_sites = codegen::collect_call_sites(document);
    let baml = codegen::generate_baml(document, &resolved, &call_sites)?;

    let baml_dir = output_root.join("generated").join("baml_src");
    write_text_file(&baml_dir.join("generators.baml"), &baml.generators)?;
    write_text_file(&baml_dir.join("clients.baml"), &baml.clients)?;
    write_text_file(&baml_dir.join("types.baml"), &baml.types)?;
    write_text_file(&baml_dir.join("functions.baml"), &baml.functions)?;

    // Optionally run `baml-cli generate` if available
    let exit_status = Command::new("npx")
        .args(["baml-cli", "generate", "--from", baml_dir.to_string_lossy().as_ref()])
        .current_dir(output_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match exit_status {
        Ok(status) if status.success() => {}
        Ok(_) => {
            eprintln!(
                "warning: `npx baml-cli generate` failed — BAML client not updated. \
                 LLM features (retries, temperature) will be degraded."
            );
        }
        Err(_) => {
            eprintln!(
                "warning: BAML runtime not found. LLM features (retries, temperature, custom providers) \
                 will be degraded. Install @boundaryml/baml for full functionality."
            );
        }
    }

    Ok(())
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

#[derive(Serialize)]
struct TestManifest<'a> {
    compiled: CompiledDocument<'a>,
    tests: Vec<&'a TestDecl>,
}

#[derive(Debug, Deserialize)]
struct TestRunnerLine {
    name: Option<String>,
    status: Option<String>,
    duration_ms: Option<u64>,
    error: Option<String>,
    node_path: Option<String>,
    summary: Option<bool>,
    passed: Option<usize>,
    failed: Option<usize>,
    total_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct TestSummaryLine {
    passed: usize,
    failed: usize,
    total_ms: u64,
}

// ─── Bootstrap helpers ──────────────────────────────────────────────────────

/// Verify that Node.js >= 22.6 is available. The gateway requires native TS
/// execution via --experimental-strip-types and node:sqlite, both of which
/// shipped in Node 22.6.0.
fn ensure_node_version() -> Result<(), OpenClawCliError> {
    let output = Command::new("node")
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let version_str = String::from_utf8_lossy(&out.stdout);
            let version = version_str.trim().trim_start_matches('v');
            let parts: Vec<u32> = version
                .split('.')
                .filter_map(|p| p.parse().ok())
                .collect();
            if parts.len() >= 2 && (parts[0] > 22 || (parts[0] == 22 && parts[1] >= 6)) {
                println!("[init] node v{} ✓", version);
                Ok(())
            } else {
                Err(OpenClawCliError::Message(format!(
                    "Node.js >= 22.6.0 required (found v{version}). \
                     The gateway uses --experimental-strip-types and node:sqlite."
                )))
            }
        }
        _ => Err(OpenClawCliError::Message(
            "Node.js is not installed. Install Node.js >= 22.6.0 from https://nodejs.org".to_owned(),
        )),
    }
}

/// Run `npm install` if `node_modules/` does not exist or is missing key deps.
fn ensure_npm_installed() -> Result<(), OpenClawCliError> {
    if Path::new("node_modules").exists() && Path::new("node_modules/.package-lock.json").exists() {
        println!("[init] npm dependencies ✓");
        return Ok(());
    }

    println!("[init] installing npm dependencies...");
    let status = Command::new("npm")
        .args(["install", "--no-audit", "--no-fund"])
        .status()
        .map_err(|error| {
            OpenClawCliError::Message(format!(
                "failed to run npm install: {error}. Is npm installed?"
            ))
        })?;

    if !status.success() {
        return Err(OpenClawCliError::Message(
            "npm install failed. Check npm output above for details.".to_owned(),
        ));
    }

    println!("[init] npm dependencies installed ✓");
    Ok(())
}

/// Copy `.env.example` → `.env` if `.env` does not exist yet.
fn ensure_env_file() -> Result<(), OpenClawCliError> {
    let env_path = Path::new(".env");
    if env_path.exists() {
        println!("[init] .env ✓");
        return Ok(());
    }

    let example = Path::new(".env.example");
    if example.exists() {
        fs::copy(example, env_path).map_err(|error| {
            OpenClawCliError::Message(format!("failed to copy .env.example → .env: {error}"))
        })?;
        println!("[init] created .env from .env.example (edit API keys before running)");
    } else {
        println!("[init] no .env.example found — skipping .env creation");
    }
    Ok(())
}

/// Create the `.claw/` state directory for SQLite, screenshots, and logs.
fn ensure_state_dir() -> Result<(), OpenClawCliError> {
    let state_dir = Path::new(".claw");
    if state_dir.exists() {
        return Ok(());
    }

    fs::create_dir_all(state_dir).map_err(|error| {
        OpenClawCliError::Message(format!("failed to create .claw/: {error}"))
    })?;

    // On Windows, mark the state directory as hidden (best-effort)
    #[cfg(windows)]
    {
        let _ = Command::new("attrib")
            .args(["+h", ".claw"])
            .status();
    }

    println!("[init] created .claw/ state directory");
    Ok(())
}

/// Run the full bootstrap sequence that `claw init` performs, but silently
/// skip steps that are already satisfied. Called by `claw dev` and
/// `claw build` so the user never has to run init manually.
fn ensure_bootstrapped() -> Result<(), OpenClawCliError> {
    ensure_node_version()?;
    ensure_npm_installed()?;
    ensure_state_dir()?;
    Ok(())
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
    use super::{
        build_shutdown_request, exit_code_for_error, resolve_build_request,
        resolve_gateway_command, BuildArgs, Cli, Commands, DevArgs, InitArgs, Language, TestArgs,
        OpenClawCliError, select_tests,
    };
    use clap::Parser;
    use clawc::ast::{Block, Document, Span, TestDecl};
    use clawc::config::{BuildLanguage, OpenClawConfig};
    use clawc::errors::CompilerError;
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_init_command() {
        let cli = Cli::parse_from(["claw", "init", "--force"]);

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
                config: PathBuf::from("claw.json"),
            },
            Some(&config),
        )
        .unwrap();

        assert_eq!(request.source, PathBuf::from("example.claw"));
        assert_eq!(request.language, BuildLanguage::Ts);
    }

    #[test]
    fn parses_dev_command_with_port() {
        let cli = Cli::parse_from(["claw", "dev", "--port", "9090"]);

        match cli.command {
            Commands::Dev(DevArgs { port, .. }) => assert_eq!(port, 9090),
            _ => panic!("expected dev command"),
        }
    }

    #[test]
    fn dev_command_defaults_to_port_8080() {
        let cli = Cli::parse_from(["claw", "dev"]);

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
                config: PathBuf::from("claw.json"),
            },
            Some(&config),
        )
        .unwrap();

        assert_eq!(request.language, BuildLanguage::Python);
    }

    #[test]
    fn build_shutdown_request_includes_auth_header_when_present() {
        let request = build_shutdown_request(8080, Some("top-secret"));

        assert!(request.contains("POST /shutdown HTTP/1.1"));
        assert!(request.contains("Host: 127.0.0.1:8080"));
        assert!(request.contains("x-claw-key: top-secret"));
    }

    #[test]
    fn resolve_gateway_command_prefers_explicit_config_executable() {
        let root = temp_test_dir("explicit-gateway");
        let executable = if cfg!(windows) {
            root.join("tools").join("claw-gateway.cmd")
        } else {
            root.join("tools").join("claw-gateway")
        };
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, "echo gateway\n").unwrap();

        let mut config = OpenClawConfig::template("example.claw");
        config.gateway.executable = Some(PathBuf::from("tools").join(executable.file_name().unwrap()));

        let command = resolve_gateway_command(&root.join("claw.json"), &config).unwrap();

        assert_eq!(
            fs::canonicalize(&command.program).unwrap(),
            fs::canonicalize(&executable).unwrap()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_gateway_command_uses_local_node_modules_binary() {
        let root = temp_test_dir("node-modules-gateway");
        let gateway = root
            .join("node_modules")
            .join(".bin")
            .join(if cfg!(windows) {
                "claw-gateway.cmd"
            } else {
                "claw-gateway"
            });
        fs::create_dir_all(gateway.parent().unwrap()).unwrap();
        fs::write(&gateway, "echo gateway\n").unwrap();

        let config = OpenClawConfig::template("example.claw");
        let command = resolve_gateway_command(&root.join("claw.json"), &config).unwrap();

        assert_eq!(
            fs::canonicalize(&command.program).unwrap(),
            fs::canonicalize(&gateway).unwrap()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn maps_parse_errors_to_exit_code_one() {
        let error = OpenClawCliError::Compiler {
            error: CompilerError::ParseError {
                message: "expected identifier".to_owned(),
                span: Span::default(),
            },
            rendered: "parse failure".to_owned(),
        };

        assert_eq!(exit_code_for_error(&error), 1);
    }

    #[test]
    fn maps_semantic_errors_to_exit_code_two() {
        let semantic_errors = [
            CompilerError::UndefinedTool {
                name: "Search".to_owned(),
                span: Span::default(),
            },
            CompilerError::UndefinedAgent {
                name: "Researcher".to_owned(),
                span: Span::default(),
            },
            CompilerError::UndefinedClient {
                name: "FastOpenAI".to_owned(),
                span: Span::default(),
            },
            CompilerError::UndefinedType {
                name: "SearchResult".to_owned(),
                span: Span::default(),
            },
            CompilerError::TypeMismatch {
                expected: "string".to_owned(),
                found: "int".to_owned(),
                span: Span::default(),
            },
            CompilerError::CircularType {
                type_name: "Node".to_owned(),
                cycle_path: vec!["Node".to_owned()],
                span: Span::default(),
            },
            CompilerError::MissingReturn {
                workflow_name: "Analyze".to_owned(),
                span: Span::default(),
            },
            CompilerError::InvalidControlFlow {
                keyword: "break".to_owned(),
                span: Span::default(),
            },
            CompilerError::InvalidAssertOutsideTest {
                span: Span::default(),
            },
            CompilerError::DuplicateSymbol {
                name: "Analyze".to_owned(),
                first_span: Span::default(),
                second_span: Span::default(),
            },
        ];

        for compiler_error in semantic_errors {
            let error = OpenClawCliError::Compiler {
                error: compiler_error,
                rendered: "semantic failure".to_owned(),
            };
            assert_eq!(exit_code_for_error(&error), 2);
        }
    }

    #[test]
    fn maps_codegen_errors_to_exit_code_three() {
        let error = OpenClawCliError::Codegen("template render failed".to_owned());

        assert_eq!(exit_code_for_error(&error), 3);
    }

    #[test]
    fn maps_io_errors_to_exit_code_four() {
        let error = OpenClawCliError::Compiler {
            error: CompilerError::Io {
                path: PathBuf::from("missing.claw"),
                source: io::Error::from(io::ErrorKind::NotFound),
            },
            rendered: "missing file".to_owned(),
        };

        assert_eq!(exit_code_for_error(&error), 4);
    }

    #[test]
    fn parses_test_command_with_source() {
        let cli = Cli::parse_from(["claw", "test", "example.claw"]);

        match cli.command {
            Commands::Test(TestArgs { source, filter, .. }) => {
                assert_eq!(source, Some(PathBuf::from("example.claw")));
                assert_eq!(filter, None);
            }
            _ => panic!("expected test command"),
        }
    }

    #[test]
    fn parses_test_filter() {
        let cli = Cli::parse_from(["claw", "test", "--filter", "Researcher"]);

        match cli.command {
            Commands::Test(TestArgs { source, filter, .. }) => {
                assert_eq!(source, None);
                assert_eq!(filter, Some("Researcher".to_owned()));
            }
            _ => panic!("expected test command"),
        }
    }

    #[test]
    fn test_filter_returns_no_matches_without_error() {
        let document = Document {
            imports: Vec::new(),
            types: Vec::new(),
            clients: Vec::new(),
            tools: Vec::new(),
            agents: Vec::new(),
            workflows: Vec::new(),
            listeners: Vec::new(),
            tests: vec![TestDecl {
                name: "Researcher returns results".to_owned(),
                body: Block {
                    statements: Vec::new(),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            mocks: Vec::new(),
            span: Span::default(),
        };

        let selected = select_tests(&document, Some("nonexistent"));
        assert!(selected.is_empty());
    }

    fn temp_test_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "claw-cli-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
