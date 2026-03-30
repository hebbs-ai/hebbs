#[cfg(feature = "grpc")]
use std::io::IsTerminal;

use clap::Parser;
use tracing_subscriber::{fmt, EnvFilter};

use hebbs_cli::cli::{Cli, ColorArg, Commands, FormatArg};
use hebbs_cli::config::{CliConfig, ColorMode, OutputFormat};

fn main() {
    let cli = Cli::parse();

    let mut config = CliConfig::load();

    if let Some(ref ep) = cli.endpoint {
        let endpoint = if ep.starts_with("http://") || ep.starts_with("https://") {
            ep.clone()
        } else {
            format!("http://{}", ep)
        };
        config.endpoint = endpoint;
    }
    if let Some(hp) = cli.http_port {
        config.http_port = hp;
    }
    if let Some(tm) = cli.timeout {
        config.timeout_ms = tm;
    }
    if let Some(ref fmt) = cli.format {
        config.output_format = match fmt {
            FormatArg::Human => OutputFormat::Human,
            FormatArg::Json => OutputFormat::Json,
            FormatArg::Raw => OutputFormat::Raw,
        };
    }
    if let Some(ref c) = cli.color {
        config.color = match c {
            ColorArg::Always => ColorMode::Always,
            ColorArg::Never => ColorMode::Never,
            ColorArg::Auto => ColorMode::Auto,
        };
    }
    if cli.tenant.is_some() {
        config.tenant = cli.tenant.clone();
    }

    init_tracing(cli.verbose);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");

    rt.block_on(async {
        let cmd = match cli.command {
            Some(cmd) => cmd,
            None => {
                // No subcommand: run REPL if gRPC available, else show help
                #[cfg(feature = "grpc")]
                {
                    let is_tty = std::io::stdout().is_terminal();
                    let use_color = config.should_color(is_tty);
                    let renderer =
                        hebbs_cli::format::Renderer::new(config.output_format, use_color);
                    let mut conn = hebbs_cli::connection::ConnectionManager::new(
                        config.endpoint.clone(),
                        config.timeout_ms,
                    )
                    .with_api_key(cli.api_key.clone());
                    hebbs_cli::repl::run_repl(
                        &mut conn,
                        &renderer,
                        &config.history_file,
                        config.max_history,
                        config.http_port,
                        config.tenant.as_deref(),
                    )
                    .await;
                    return;
                }
                #[cfg(not(feature = "grpc"))]
                {
                    eprintln!("Usage: hebbs <command>");
                    eprintln!("Run 'hebbs --help' for available commands.");
                    eprintln!("Run 'hebbs login --endpoint <url>' to connect to a server.");
                    std::process::exit(1);
                }
            }
        };

        // Login is always handled by REST (it's a remote-only command)
        #[cfg(feature = "rest")]
        if matches!(cmd, Commands::Login { .. }) {
            let exit_code = hebbs_cli::rest::execute_rest(
                cmd,
                &config,
                cli.api_key.clone(),
                config.output_format,
            )
            .await;
            std::process::exit(exit_code);
        }

        // Determine transport: REST or gRPC
        #[cfg(feature = "rest")]
        let use_rest = hebbs_cli::rest::is_rest_endpoint(&config.endpoint);
        #[cfg(not(feature = "rest"))]
        let use_rest = false;

        // REST-only commands always use REST
        #[cfg(feature = "rest")]
        let use_rest = use_rest
            || matches!(
                cmd,
                Commands::Push { .. }
                    | Commands::Workspaces(_)
                    | Commands::Keys(_)
                    | Commands::Dashboard
            );

        if use_rest {
            #[cfg(feature = "rest")]
            {
                let exit_code = hebbs_cli::rest::execute_rest(
                    cmd,
                    &config,
                    cli.api_key.clone(),
                    config.output_format,
                )
                .await;
                std::process::exit(exit_code);
            }
            #[cfg(not(feature = "rest"))]
            {
                eprintln!("REST transport not available in this build.");
                eprintln!("Install with REST support or use a local daemon on port 6380.");
                std::process::exit(1);
            }
        }

        // gRPC mode (local daemon)
        #[cfg(feature = "grpc")]
        {
            let is_tty = std::io::stdout().is_terminal();
            let use_color = config.should_color(is_tty);
            let renderer = hebbs_cli::format::Renderer::new(config.output_format, use_color);
            let mut conn = hebbs_cli::connection::ConnectionManager::new(
                config.endpoint.clone(),
                config.timeout_ms,
            )
            .with_api_key(cli.api_key.clone());
            let tenant_id = config.tenant.as_deref();
            let exit_code = hebbs_cli::commands::execute(
                cmd,
                &mut conn,
                &renderer,
                config.http_port,
                tenant_id,
            )
            .await;
            std::process::exit(exit_code);
        }
        #[cfg(not(feature = "grpc"))]
        {
            eprintln!("gRPC transport not available in this build.");
            eprintln!("Connect to a remote server: hebbs login --endpoint <url>");
            std::process::exit(1);
        }
    });
}

fn init_tracing(verbosity: u8) {
    let level = match verbosity {
        0 => return,
        1 => "debug",
        _ => "trace",
    };

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}
