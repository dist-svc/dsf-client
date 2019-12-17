
use std::io;

extern crate structopt;
use structopt::StructOpt;

extern crate futures;

extern crate async_std;
use async_std::task;

#[macro_use] extern crate log;
extern crate simplelog;
use simplelog::{TermLogger, LevelFilter};

extern crate dsf_core;

extern crate dsf_client;
use dsf_client::{Client};

extern crate dsf_rpc;
use dsf_rpc::{RequestKind, ResponseKind};

extern crate humantime;
use humantime::Duration;

#[derive(StructOpt)]
#[structopt(name = "DSF Client", about = "Distributed Service Discovery (DSF) client, interacts with the DSF daemon to publish and locate services")]
struct Config {

    #[structopt(subcommand)]
    cmd: RequestKind,

    #[structopt(short = "d", long = "daemon-socket", default_value = "/tmp/dsf.sock")]
    /// Specify the socket to bind the DSF daemon
    daemon_socket: String,

    #[structopt(long = "log-level", default_value = "info")]
    /// Enable verbose logging
    level: LevelFilter,

    #[structopt(long, default_value = "3s")]
    timeout: Duration,
}

fn main() -> Result<(), io::Error> {
    // Fetch arguments
    let opts = Config::from_args();

    // Setup logging
    let mut log_config = simplelog::ConfigBuilder::new();
    log_config.add_filter_ignore_str("tokio");
    let log_config = log_config.build();

    let _ = TermLogger::init(opts.level, log_config, simplelog::TerminalMode::Mixed).unwrap();

    task::block_on(async {
        // Create client connector
        debug!("Connecting to client socket: '{}'", &opts.daemon_socket);
        let mut c = match Client::new(&opts.daemon_socket, *opts.timeout) {
            Ok(c) => c,
            Err(e) => {
                error!("Error connecting to daemon on '{}': {:?}", &opts.daemon_socket, e);
                return
            }
        };

        // Execute request and handle response
        let res = c.request(opts.cmd).await;

        match res {
            Ok(resp) => {
                handle_response(resp);
            },
            Err(e) => {
                error!("error: {:?}", e);
            }
        }

    });

    Ok(())
}

fn handle_response(resp: ResponseKind) {
    match resp {
        ResponseKind::Created(info) => {
            println!("Created:");
            println!("{:+}", info);
        },
        ResponseKind::Services(services) => {
            println!("Services:");
            for s in &services {
                println!("{:+}", s);
            }
        },
        ResponseKind::Peers(peers) => {
            println!("Peers:");
            for (_id, p) in &peers {
                println!("{:+}", p);
            }
        },
        ResponseKind::Data(data) => {
            println!("Data:");
            for d in &data {
                println!("{:+}", d);
            }
        }
        ResponseKind::Error(e) => error!("{:?}", e),
        ResponseKind::Unrecognised => warn!("command not yet implemented"),
        _ => warn!("unhandled response: {:?}", resp),
    }
}