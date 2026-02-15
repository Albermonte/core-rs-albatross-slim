use std::time::Duration;

use futures::StreamExt as _;
use log::info;
pub use nimiq::{
    client::Client,
    config::{command_line::CommandLine, config::ClientConfig, config_file::ConfigFile},
    error::Error,
    extras::{
        logging::{initialize_logging, log_error_cause_chain},
        panic::initialize_panic_reporting,
        signal_handling::initialize_signal_handler,
    },
};
use nimiq_time::interval;
use nimiq_utils::spawn;

async fn main_inner() -> Result<(), Error> {
    // Parse command line.
    let command_line = CommandLine::parse();
    log::trace!("Command line: {:#?}", command_line);

    // Parse config file - this will obey the `--config` command line option.
    let config_file = ConfigFile::find(Some(&command_line))?;
    log::trace!("Config file: {:#?}", config_file);

    // Initialize logging with config values.
    initialize_logging(Some(&command_line), Some(&config_file.log))?;

    // Initialize panic hook.
    initialize_panic_reporting();

    // Initialize signal handler
    initialize_signal_handler();

    // Create config builder and apply command line and config file.
    let mut builder = ClientConfig::builder();
    builder.config_file(&config_file)?;
    builder.command_line(&command_line)?;

    // Finalize config.
    let config = builder.build()?;
    log::debug!("Final configuration: {:#?}", config);

    // Clone config for RPC
    let rpc_config = config.rpc_server.clone();

    // Create client from config.
    let mut client: Client = Client::from_config(config).await?;

    // Initialize RPC server
    if let Some(rpc_config) = rpc_config {
        use nimiq::extras::rpc_server::initialize_rpc_server;
        let rpc_server =
            initialize_rpc_server(&client, rpc_config).expect("Failed to initialize RPC server");
        spawn(async move { rpc_server.run().await });
    }

    // Start consensus.
    let consensus = client.take_consensus().unwrap();
    spawn(consensus);
    let consensus = client.consensus_proxy();

    let zkp_component = client.take_zkp_component().unwrap();
    spawn(zkp_component);

    // Create the "monitor" future which never completes to keep the client alive.
    let mut statistics_interval = config_file.log.statistics;
    let mut show_statistics = true;
    if statistics_interval == 0 {
        statistics_interval = 10;
        show_statistics = false;
    }

    // Run periodically
    let mut interval = interval(Duration::from_secs(statistics_interval));
    loop {
        interval.next().await;

        if show_statistics {
            match client.network().network_info().await {
                Ok(network_info) => {
                    let head = client.blockchain_head();

                    info!(
                        consensus_established = consensus.is_established(),
                        block_number = head.block_number(),
                        num_peers = network_info.num_peers(),
                        "Consensus: {} - Head: {} - Peers: {}",
                        if consensus.is_established() {
                            "established"
                        } else {
                            "lost"
                        },
                        head,
                        network_info.num_peers(),
                    )
                }
                Err(err) => {
                    log::error!("Error retrieving NetworkInfo: {:?}", err);
                }
            };
        }
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = main_inner().await {
        log_error_cause_chain(&e);
        std::process::exit(1);
    }
    std::process::exit(0);
}
