use std::collections::HashSet;

use nimiq_jsonrpc_server::{
    AllowListDispatcher, Config, Cors, Credentials, ModularDispatcher, Server as _Server,
};
use nimiq_rpc_server::dispatchers::*;

#[cfg(feature = "rpc-server")]
use crate::config::config::RpcServerConfig;
use crate::{client::Client, config::consts::default_bind, error::Error};

pub type Server = _Server<AllowListDispatcher<ModularDispatcher>>;

/// The 11 RPC methods this lightweight history node exposes.
const ALLOWED_METHODS: &[&str] = &[
    "getAccountByAddress",
    "getTransactionsByAddress",
    "getTransactionByHash",
    "getTransactionsByBlockNumber",
    "getTransactionsByBatchNumber",
    "getBlockNumber",
    "getBatchNumber",
    "getBlockByNumber",
    "isConsensusEstablished",
    "subscribeForHeadBlock",
    "subscribeForHeadBlockHash",
];

#[cfg(feature = "rpc-server")]
pub fn initialize_rpc_server(client: &Client, config: RpcServerConfig) -> Result<Server, Error> {
    let ip = config.bind_to.unwrap_or_else(default_bind);
    log::info!("Initializing RPC server: {}:{}", ip, config.port);

    // Configure RPC server
    let basic_auth = config.credentials.map(|credentials| {
        Credentials::new_from_blake2b(credentials.username, credentials.password_hash.0 .0)
    });

    let allowed_methods: HashSet<String> = ALLOWED_METHODS.iter().map(|s| (*s).to_string()).collect();

    let cors_domains = config.cors_domains.unwrap_or_default();
    let is_cors_wildcard = cors_domains.iter().any(|origin| origin.trim() == "*");
    let cors_config = if is_cors_wildcard {
        Cors::new().with_any_origin()
    } else {
        Cors::new().with_origins(cors_domains)
    };

    let mut dispatcher = ModularDispatcher::default();

    dispatcher.add(BlockchainDispatcher::new(client.blockchain()));
    dispatcher.add(ConsensusDispatcher::new(client.consensus_proxy().established_flag()));

    Ok(Server::new(
        Config {
            bind_to: (config.bind_to.unwrap_or_else(default_bind), config.port).into(),
            enable_websocket: true,
            ip_whitelist: None,
            basic_auth,
            cors: Some(cors_config),
        },
        AllowListDispatcher::new(dispatcher, Some(allowed_methods)),
    ))
}
