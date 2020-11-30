/* Copyright 2020 The TensorFlow Authors. All Rights Reserved.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
==============================================================================*/

use clap::Clap;
use log::{debug, info, LevelFilter};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tonic::transport::Server;

use rustboard_core::commit::Commit;
use rustboard_core::logdir::LogdirLoader;
use rustboard_core::proto::tensorboard::data;
use rustboard_core::server::DataProviderHandler;

use data::tensor_board_data_provider_server::TensorBoardDataProviderServer;

#[derive(Clap, Debug)]
#[clap(name = "rustboard", version = "0.1.0")]
struct Opts {
    /// Log directory to load
    ///
    /// Directory to recursively scan for event files (files matching the `*tfevents*` glob). This
    /// directory, its descendants, and its event files will be periodically polled for new data.
    #[clap(long)]
    logdir: PathBuf,

    /// Bind to this IP address
    ///
    /// IP address to bind this server to. May be an IPv4 address (e.g., 127.0.0.1 or 0.0.0.0) or
    /// an IPv6 address (e.g., ::1 or ::0).
    #[clap(long, default_value = "::0")]
    host: IpAddr,

    /// Bind to this port
    ///
    /// Port to bind this server to. Use `0` to request an arbitrary free port from the OS.
    #[clap(long, default_value = "6806")]
    port: u16,

    /// Delay between reload cycles (seconds)
    ///
    /// Number of seconds to wait between finishing one load cycle and starting the next one. This
    /// does not include the time for the reload itself.
    #[clap(long, default_value = "5")]
    reload_interval: Seconds,

    /// Use verbose output (-vv for very verbose output)
    #[clap(long = "verbose", short, parse(from_occurrences))]
    verbosity: u32,
}

/// A duration in seconds.
#[derive(Debug, Copy, Clone)]
struct Seconds(u64);
impl FromStr for Seconds {
    type Err = <u64 as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Seconds)
    }
}
impl Seconds {
    fn duration(self) -> Duration {
        Duration::from_secs(self.0)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opts::parse();
    init_logging(match opts.verbosity {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        _ => LevelFilter::max(),
    });
    debug!("Parsed options: {:?}", opts);

    let addr = SocketAddr::new(opts.host, opts.port);
    let listener = TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    eprintln!("listening on {:?}", bound);

    // Leak the commit object, since the Tonic server must have only 'static references. This only
    // leaks the outer commit structure (of constant size), not the pointers to the actual data.
    let commit: &'static Commit = Box::leak(Box::new(Commit::new()));

    std::thread::spawn(move || {
        let mut loader = LogdirLoader::new(commit, opts.logdir);
        loop {
            info!("Starting load cycle");
            let start = Instant::now();
            loader.reload();
            let end = Instant::now();
            info!("Finished load cycle ({:?})", end - start);
            std::thread::sleep(opts.reload_interval.duration());
        }
    });

    let handler = DataProviderHandler { commit };
    Server::builder()
        .add_service(TensorBoardDataProviderServer::new(handler))
        .serve_with_incoming(listener)
        .await?;
    Ok(())
}

/// Installs a logging handler whose behavior is determined by the `RUST_LOG` environment variable
/// (per <https://docs.rs/env_logger> semantics), or by including all logs at `default_log_level`
/// or above if `RUST_LOG_LEVEL` is not given.
fn init_logging(default_log_level: LevelFilter) {
    use env_logger::{Builder, Env};
    Builder::from_env(Env::default().default_filter_or(default_log_level.to_string())).init();
}
