
use std::io;

extern crate structopt;
use structopt::StructOpt;
use structopt::clap::Shell;

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

extern crate chrono;
extern crate chrono_humanize;

#[macro_use] extern crate prettytable;
use prettytable::{Table};


#[derive(StructOpt)]
#[structopt(name = "DSF Client", about = "Distributed Service Discovery (DSF) client, interacts with the DSF daemon to publish and locate services")]
struct Config {

    #[structopt(subcommand)]
    cmd: Commands,

    #[structopt(short = "d", long = "daemon-socket", default_value = "/tmp/dsf.sock", env="DSF_SOCK")]
    /// Specify the socket to bind the DSF daemon
    daemon_socket: String,

    #[structopt(long = "log-level", default_value = "info")]
    /// Enable verbose logging
    level: LevelFilter,

    #[structopt(long, default_value = "3s")]
    timeout: Duration,
}

#[derive(StructOpt)]
enum Commands {
    /// Flattened enum for DSF requests
    #[structopt(flatten)]
    Request(RequestKind),
    
    /// Generate shell completion information
    Completion{
        #[structopt(long, default_value = "zsh")]
        /// Shell for completion file output
        shell: Shell,

        #[structopt(long, default_value = "./")]
        /// Completion file output directory
        dir: String,
    },
}

fn main() -> Result<(), io::Error> {
    // Fetch arguments
    let opts = Config::from_args();

    // Setup logging
    let mut log_config = simplelog::ConfigBuilder::new();
    log_config.add_filter_ignore_str("tokio");
    let log_config = log_config.build();

    let _ = TermLogger::init(opts.level, log_config, simplelog::TerminalMode::Mixed).unwrap();

    // Parse out commands
    let cmd = match &opts.cmd {
        Commands::Request(r) => r,
        Commands::Completion{shell, dir} => {
            info!("Writing completions for {} to: {}", *shell, dir);
            Config::clap().gen_completions("dsfc", *shell, &dir);
            return Ok(())
        }
    };

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
        let res = c.request(cmd.clone()).await;

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
        ResponseKind::Service(info) => {
            println!("Created / Located service");
            print_services(&[info]);
        },
        ResponseKind::Services(services) => {
            print_services(&services);
        },
        ResponseKind::Peers(peers) => {
            print_peers(&peers);
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

use dsf_core::types::Id;
use dsf_rpc::{PeerInfo, ServiceInfo};

use std::time::SystemTime;

fn systemtime_to_humantime(s: SystemTime) -> String {
    let v = chrono::DateTime::<chrono::Local>::from(s);
    chrono_humanize::HumanTime::from(v).to_string()
}

fn print_peers(peers: &[(Id, PeerInfo)]) {
    // Create the table
    let mut table = Table::new();

    table.set_format(*prettytable::format::consts::FORMAT_CLEAN);

    // Add a row per time
    table.add_row(row![b => "Peer ID", "Index", "State", "Address(es)", "Seen", "Sent", "Received", "Blocked"]);
    
    for (_id, p) in peers {
        table.add_row(row![
            p.id.to_string(),
            p.index.to_string(),
            p.state.to_string(),
            format!("{}", p.address()),
            p.seen.map(systemtime_to_humantime).unwrap_or( "Never".to_string() ),
            format!("{}", p.sent),
            format!("{}", p.received),
            //format!("{}", p.blocked),
        ]);
    }

    // Print the table to stdout
    table.printstd();
}

fn print_services(services: &[ServiceInfo]) {
    // Create the table
    let mut table = Table::new();

    table.set_format(*prettytable::format::consts::FORMAT_CLEAN);

    // Add a row per time
    table.add_row(row![b => "Service ID", "Index", "State", "Updated", "PublicKey", "PrivateKey", "SecretKey", "Subscribers", "Replicas"]);
    
    for s in services {
        table.add_row(row![
            s.id.to_string(),
            s.index.to_string(),
            s.state.to_string(),
            s.last_updated.map(systemtime_to_humantime).unwrap_or( "Never".to_string() ),

            s.public_key.to_string(),
            s.private_key.as_ref().map(|_| "True".to_string()).unwrap_or("False".to_string()),
            s.secret_key.as_ref().map(|_| "True".to_string()).unwrap_or("False".to_string()),

            format!("{}", s.subscribers),
            format!("{}", s.replicas),
        ]);
    }

    // Print the table to stdout
    table.printstd();
}