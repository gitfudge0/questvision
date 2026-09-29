mod adaptive;
mod audio;
mod capture;
mod config;
mod gui;
mod macos_permissions;
mod monitor;
mod quality;
mod rtc;
mod security;
mod server;
mod virtual_display;
use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;
use std::net::IpAddr;

#[derive(Parser)]
#[command(
    name = "questdisplay",
    version,
    about = "LAN browser display streaming host"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(long, global = true)]
    listen: Option<IpAddr>,
    #[arg(long, global = true)]
    port: Option<u16>,
    #[arg(long, global = true)]
    audio: bool,
    #[arg(long, global = true, default_value = "error,questdisplay=info")]
    log_level: String,
}
#[derive(Subcommand)]
enum Command {
    Start,
    /// Open the native host dashboard with an embedded runtime.
    Gui,
    Doctor,
    Displays,
    Config,
    Devices {
        #[command(subcommand)]
        action: Option<DeviceCommand>,
    },
    Virtual {
        #[command(subcommand)]
        action: VirtualCommand,
    },
    Benchmark {
        #[arg(long, default_value = "balanced", value_parser = ["performance", "balanced", "quality"])]
        preset: String,
    },
}

#[derive(Subcommand)]
enum DeviceCommand {
    List,
    Revoke { id: String },
    RevokeAll,
}

#[derive(Subcommand)]
enum VirtualCommand {
    Status,
    List,
    Create,
}

fn resolve_command(command: Option<Command>) -> Command {
    command.unwrap_or({
        if cfg!(target_os = "macos") {
            Command::Gui
        } else {
            Command::Start
        }
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(&cli.log_level)
        .init();
    let mut config = config::load()?;
    if let Some(listen) = cli.listen {
        config.listen = listen;
    }
    if let Some(port) = cli.port {
        config.port = port;
    }
    if cli.audio {
        config.audio = true;
    }
    config.validate()?;
    match resolve_command(cli.command) {
        Command::Start => server::serve(config).await?,
        Command::Gui => {
            let host = gui::run(config, tokio::runtime::Handle::current())?;
            host.shutdown().await;
        }
        Command::Doctor => doctor(&config),
        Command::Displays => println!(
            "System-selected primary display. Linux portals may require choosing a monitor when streaming starts."
        ),
        Command::Config => {
            let count = config.paired_tokens.len();
            config.paired_tokens.clear();
            println!("{}", toml::to_string_pretty(&config)?);
            println!("Paired credentials: {count} (redacted)");
        }
        Command::Devices { action } => devices_command(&mut config, action)?,
        Command::Benchmark { preset } => benchmark(&config, &preset).await?,
        Command::Virtual { action } => virtual_command(action).await?,
    }
    Ok(())
}
fn doctor(config: &Config) {
    println!("Quest Display Doctor");
    println!("OS: {}", std::env::consts::OS);
    println!("LAN listen: {}:{}", config.listen, config.port);
    #[cfg(target_os = "linux")]
    {
        println!(
            "Session: {}",
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                "Wayland"
            } else if std::env::var_os("DISPLAY").is_some() {
                "X11"
            } else {
                "no desktop detected"
            }
        );
        println!(
            "PipeWire: {}",
            if std::path::Path::new("/run/user").exists() {
                "check portal permission on first capture"
            } else {
                "not detected"
            }
        );
    }
    let virtual_capability = virtual_display::capabilities();
    println!("Virtual display: {}", virtual_capability.reason);
    println!("Capture and H.264 encoding are checked when a browser starts streaming.");
}

async fn benchmark(config: &Config, preset: &str) -> Result<()> {
    use std::time::{Duration, Instant};
    println!("Opening native capture. Approve the OS picker if it appears.");
    let mut stream = capture::start(
        quality::StreamSettings::from_offer(
            config,
            Some(preset),
            Some(config.fps),
            Some(config.bitrate_mbps),
        )
        .map_err(anyhow::Error::msg)?,
        &config.display,
        false,
    )
    .await?;
    let mut bytes = 0usize;
    let mut resize_time = Duration::ZERO;
    let mut color_convert_time = Duration::ZERO;
    let mut encode_time = Duration::ZERO;
    let mut output_size = None;
    let started = Instant::now();
    for _ in 0..120 {
        let frame = tokio::time::timeout(Duration::from_secs(5), stream.video.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("capture stopped during benchmark"))?;
        bytes += frame.data.len();
        resize_time += frame.resize_time;
        color_convert_time += frame.color_convert_time;
        encode_time += frame.encode_time;
        output_size = Some(frame.output_size);
    }
    let elapsed = started.elapsed().as_secs_f64();
    println!("Measured frames: 120");
    if let Some((width, height)) = output_size {
        println!("Encoded size: {width}x{height}");
    }
    println!(
        "Mean resize time: {:.2} ms",
        resize_time.as_secs_f64() * 1000.0 / 120.0
    );
    println!(
        "Mean BGRA/RGBA to YUV time: {:.2} ms",
        color_convert_time.as_secs_f64() * 1000.0 / 120.0
    );
    println!("Captured/encoded FPS: {:.1}", 120.0 / elapsed);
    println!(
        "Mean H.264 encode time: {:.2} ms",
        encode_time.as_secs_f64() * 1000.0 / 120.0
    );
    println!(
        "Mean encoded bitrate: {:.2} Mbps",
        bytes as f64 * 8.0 / elapsed / 1_000_000.0
    );
    println!("Capture API time and end-to-end display latency are not measured by this benchmark.");
    Ok(())
}

async fn virtual_command(action: VirtualCommand) -> Result<()> {
    match action {
        VirtualCommand::Status => {
            let capability = virtual_display::capabilities();
            println!("Virtual display available: {}", capability.available);
            println!("{}", capability.reason);
        }
        VirtualCommand::List => {
            for display in virtual_display::list()? {
                println!(
                    "{} ({:?}, owned by this process: {})",
                    display.name, display.compositor, display.owned
                );
            }
        }
        VirtualCommand::Create => {
            let display = virtual_display::create()?;
            println!(
                "Created virtual display {}. Keep this command running while using it. Press Ctrl-C to remove it.",
                display.name
            );
            wait_for_shutdown().await?;
            virtual_display::remove(&display.name)?;
            println!("Removed virtual display {}.", display.name);
        }
    }
    Ok(())
}

async fn wait_for_shutdown() -> Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

fn devices_command(config: &mut Config, action: Option<DeviceCommand>) -> Result<()> {
    match action.unwrap_or(DeviceCommand::List) {
        DeviceCommand::List => {
            let ids = config.device_ids();
            if ids.is_empty() {
                println!("No paired browser credentials.");
            }
            for id in ids {
                println!("{id}");
            }
        }
        DeviceCommand::Revoke { id } => {
            config.remove_device(&id)?;
            config.save()?;
            println!("Revoked device {id}.");
        }
        DeviceCommand::RevokeAll => {
            let count = config.paired_tokens.len();
            config.paired_tokens.clear();
            config.save()?;
            println!("Revoked {count} paired browser credential(s).");
        }
    }
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn no_subcommand_uses_the_platform_default() {
        let cli = Cli::try_parse_from(["questdisplay"]).unwrap();
        let command = resolve_command(cli.command);
        if cfg!(target_os = "macos") {
            assert!(matches!(command, Command::Gui));
        } else {
            assert!(matches!(command, Command::Start));
        }
    }

    #[test]
    fn explicit_start_selects_the_cli_host() {
        let cli = Cli::try_parse_from(["questdisplay", "start"]).unwrap();
        assert!(matches!(resolve_command(cli.command), Command::Start));
    }
}
