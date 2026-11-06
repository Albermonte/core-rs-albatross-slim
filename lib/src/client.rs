use std::{
    fs, io,
    num::NonZeroU8,
    sync::{atomic::AtomicU32, Arc},
};

use instant::SystemTime;
use nimiq_block::Block;
#[cfg(feature = "full-consensus")]
use nimiq_blockchain::{Blockchain, BlockchainConfig};
use nimiq_blockchain_interface::AbstractBlockchain;
use nimiq_blockchain_proxy::BlockchainProxy;
#[cfg(feature = "full-consensus")]
use nimiq_consensus::Error::BlockchainError;
use nimiq_consensus::{
    sync::syncer_proxy::SyncerProxy, BlsCache, Consensus as AbstractConsensus,
    ConsensusProxy as AbstractConsensusProxy,
};
#[cfg(feature = "full-consensus")]
use nimiq_dht::Verifier;
use nimiq_genesis::NetworkInfo;
use nimiq_light_blockchain::LightBlockchain;
use nimiq_network_interface::{
    network::Network as NetworkInterface,
    peer_info::{NodeType, Services},
    Multiaddr, Protocol,
};
use nimiq_network_libp2p::{
    discovery::peer_contacts::PeerContact, Config as NetworkConfig, Network,
    TlsConfig as NetworkTls,
};
use nimiq_primitives::policy::Policy;
#[cfg(feature = "full-consensus")]
use nimiq_utils::time::OffsetTime;
use parking_lot::{Mutex, RwLock};
use rustls_pemfile::Item;

use crate::{
    config::config::{ClientConfig, SyncMode},
    error::Error,
};

/// Alias for the Consensus and Validator specialized over libp2p network
pub type Consensus = AbstractConsensus<Network>;
pub type ConsensusProxy = AbstractConsensusProxy<Network>;


/// Holds references to the relevant structs. This is then Arc'd in `Client` and a nice API is
/// exposed.
pub(crate) struct ClientInner {
    network: Arc<Network>,

    /// The consensus object, which maintains the blockchain, the network and other things to
    /// reach consensus.
    consensus: ConsensusProxy,

    blockchain: BlockchainProxy,

}

/// This function is used to generate the services flags (provided, needed) based upon the configured sync mode
pub fn generate_service_flags(sync_mode: SyncMode, index_history: bool) -> (Services, Services) {
    let provided_services = match sync_mode {
        // Services provided by history nodes
        SyncMode::History => {
            log::info!("Client configured as a history node");
            let mut services = Services::provided(NodeType::History);
            if index_history {
                services |= Services::TRANSACTION_INDEX;
            }
            services
        }
        // Services provided by full nodes
        SyncMode::Full => {
            log::info!("Client configured as a full node");
            Services::provided(NodeType::Full)
        }
        // Services provided by light nodes
        SyncMode::Light => {
            log::info!("Client configured as a light node");
            Services::provided(NodeType::Light)
        }
        SyncMode::Pico => {
            log::info!("Client configured as a pico node");
            Services::provided(NodeType::Pico)
        }
    };

    let required_services = match sync_mode {
        // Services required by history nodes
        SyncMode::History => Services::required(NodeType::History),
        // Services required by full nodes
        SyncMode::Full => Services::required(NodeType::Full),
        // Services required by light nodes
        SyncMode::Light => Services::required(NodeType::Light),
        SyncMode::Pico => Services::required(NodeType::Pico),
    };
    (provided_services, required_services)
}

impl ClientInner {
    async fn from_config(config: ClientConfig) -> Result<Client, Error> {
        // Get network info (i.e. which specific blockchain we're on)
        if !config.network_id.is_albatross() {
            return Err(Error::config_error(format!(
                "{} is not compatible with Albatross",
                config.network_id
            )));
        }
        let network_info = NetworkInfo::from_network_id(config.network_id);

        let policy_config = Policy {
            genesis_block_number: network_info.genesis_block().block_number(),
            max_supported_version: network_info.max_supported_version(),
            ..Policy::default_for_network(config.network_id)
        };

        let _ = Policy::get_or_init(policy_config);

        // Verify Policy is configured with the genesis block number we expect
        if network_info.genesis_block().block_number() != Policy::genesis_block_number() {
            log::error!("The genesis block number must be configured before using any other Policy function");
            return Err(Error::config_error(
                "There is a genesis block number configuration mismatch",
            ));
        }

        #[cfg(feature = "full-consensus")]
        // Initialize clock
        let time = Arc::new(OffsetTime::new());

        // Load identity keypair from file store
        let identity_keypair = config.storage.identity_keypair()?;
        log::info!("Identity public key: {:?}", identity_keypair.public());
        log::info!(
            "PeerId: {:}",
            identity_keypair.public().to_peer_id().to_base58()
        );

        let (provided_services, required_services) =
            generate_service_flags(config.consensus.sync_mode, config.consensus.index_history);

        // Generate my peer contact from identity keypair, our own addresses
        // (from the configured advertised addresses) and my provided services
        // Filter out unspecified IP addresses since those are not addresses suitable
        // for the contact book (for others to contact ourself).
        let mut peer_contact_addresses = config
            .network
            .advertised_addresses
            .clone()
            .unwrap_or_default();
        peer_contact_addresses.retain(|address| {
            let mut protocols = address.iter();
            match protocols.next() {
                Some(Protocol::Ip4(ip)) => !ip.is_unspecified(),
                Some(Protocol::Ip6(ip)) => !ip.is_unspecified(),
                _ => true,
            }
        });

        // Set pre-genesis flag.
        #[cfg(feature = "database-storage")]
        let mut provided_services = provided_services;
        #[cfg(feature = "database-storage")]
        if config.storage.has_pre_genesis_database(config.network_id) {
            provided_services |= Services::PRE_GENESIS_TRANSACTIONS;
        }

        let peer_contact = PeerContact::new(
            peer_contact_addresses,
            identity_keypair.public(),
            provided_services,
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        )
        .map_err(|e| Error::Network(nimiq_network_libp2p::NetworkError::PeerContactError(e)))?;

        let seeds: Vec<Multiaddr> = config
            .network
            .seeds
            .clone()
            .into_iter()
            .map(|seed| seed.address)
            .collect();

        let tls_config = if let Some(tls_config) = config.network.tls {
            // Check that the provided private key has the expected format and convert the PEM file to DER format.
            let private_key = fs::read(tls_config.private_key).and_then(|private_key_bytes| {
                match rustls_pemfile::read_one(&mut &*private_key_bytes)? {
                    Some(Item::Sec1Key(key)) => Ok(key.secret_sec1_der().to_vec()),
                    Some(Item::Pkcs8Key(key)) => Ok(key.secret_pkcs8_der().to_vec()),
                    Some(Item::Pkcs1Key(key)) => Ok(key.secret_pkcs1_der().to_vec()),
                    _ => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid TLS private key",
                    )),
                }
            })?;
            // Check that the provided certificates have the expected format and convert the PEM file to a list of
            // certificates in DER format.
            // We could have several certificates in the same file, read them all and build the array of certificates
            // that the network requires.
            let certificates = fs::read(tls_config.certificates).and_then(|certificate_bytes| {
                rustls_pemfile::read_all(&mut &*certificate_bytes)
                    .map(|item| match item {
                        Ok(Item::X509Certificate(cert)) => Ok(cert.to_vec()),
                        _ => Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Invalid TLS certificate(s)",
                        )),
                    })
                    .collect()
            })?;
            Some(NetworkTls {
                private_key,
                certificates,
            })
        } else {
            None
        };

        // Setup libp2p network
        let network_config = NetworkConfig::new(
            identity_keypair,
            peer_contact,
            seeds,
            network_info.genesis_hash().clone(),
            false,
            required_services,
            tls_config,
            config.network.desired_peer_count,
            config.network.peer_count_max,
            config.network.peer_count_per_ip_max,
            config.network.peer_count_per_subnet_max,
            config.network.only_secure_ws_connections,
            config.network.num_initial_connections,
            config.network.allow_loopback_addresses,
            config
                .network
                .dht_quorum
                .unwrap_or(NonZeroU8::new(3).unwrap()),
            config.network.network_buffer_size,
        );

        log::debug!(
            addresses = ?config.network.listen_addresses,
            "Listen addresses");
        log::debug!(
            addresses = ?config.network.advertised_addresses,
            "Advertised addresses",
        );

        // We update the services flags depending on the pre-genesis database file being present
        #[cfg(feature = "database-storage")]
        let pre_genesis_environment = if config.storage.has_pre_genesis_database(config.network_id)
        {
            Some(
                config
                    .storage
                    .pre_genesis_database(config.network_id, config.database.clone())?,
            )
        } else {
            None
        };

        // Open database
        #[cfg(feature = "database-storage")]
        let environment = config.storage.database(
            config.network_id,
            config.consensus.sync_mode,
            config.database,
        )?;

        let bls_cache = Arc::new(Mutex::new(BlsCache::default()));

        #[cfg(feature = "full-consensus")]
        let blockchain_config = BlockchainConfig {
            max_epochs_stored: config.consensus.max_epochs_stored,
            keep_history: config.consensus.sync_mode == SyncMode::History,
            index_history: config.consensus.index_history,
            ..Default::default()
        };

        let blockchain_proxy = match config.consensus.sync_mode {
            #[cfg(not(feature = "full-consensus"))]
            SyncMode::History => {
                panic!("Can't build a history node without the full-consensus feature enabled")
            }
            #[cfg(not(feature = "full-consensus"))]
            SyncMode::Full => {
                panic!("Can't build a full node without the full-consensus feature enabled")
            }
            #[cfg(feature = "full-consensus")]
            SyncMode::History | SyncMode::Full => {
                let blockchain = match Blockchain::new_merged(
                    environment.clone(),
                    pre_genesis_environment,
                    blockchain_config,
                    config.network_id,
                    time,
                ) {
                    Ok(blockchain) => Arc::new(RwLock::new(blockchain)),
                    Err(err) => {
                        return Err(Error::Consensus(BlockchainError(err)));
                    }
                };
                BlockchainProxy::from(&blockchain)
            }
            SyncMode::Light | SyncMode::Pico => BlockchainProxy::from(&Arc::new(RwLock::new(
                LightBlockchain::new(config.network_id),
            ))),
        };

        // Create the Dht verifier
        #[cfg(feature = "full-consensus")]
        let dht_verifier = Verifier::new(blockchain_proxy.clone());

        // Create the network.
        let network = Arc::new(
            Network::new(
                network_config,
                #[cfg(feature = "full-consensus")]
                dht_verifier,
            )
            .await,
        );

        // Start buffering network events as early as possible
        let network_events = network.subscribe_events();

        let syncer_tracker = Arc::new(AtomicU32::new(0));

        let syncer_proxy = match config.consensus.sync_mode {
            #[cfg(not(feature = "full-consensus"))]
            SyncMode::History => {
                panic!("Can't build a history node without the full-consensus feature enabled")
            }
            #[cfg(not(feature = "full-consensus"))]
            SyncMode::Full => {
                panic!("Can't build a full node without the full-consensus feature enabled")
            }
            #[cfg(feature = "full-consensus")]
            SyncMode::History => {
                SyncerProxy::new_history(
                    blockchain_proxy.clone(),
                    Arc::clone(&network),
                    bls_cache,
                    network_events,
                )
                .await
            }
            #[cfg(feature = "full-consensus")]
            SyncMode::Full => {
                SyncerProxy::new_full(
                    blockchain_proxy.clone(),
                    Arc::clone(&network),
                    bls_cache,
                    network_events,
                    config.consensus.full_sync_threshold,
                    Arc::clone(&syncer_tracker),
                )
                .await
            }
            SyncMode::Light => {
                SyncerProxy::new_light(
                    blockchain_proxy.clone(),
                    Arc::clone(&network),
                    bls_cache,
                    network_events,
                )
                .await
            }
            SyncMode::Pico => {
                SyncerProxy::new_pico(
                    blockchain_proxy.clone(),
                    Arc::clone(&network),
                    bls_cache,
                    network_events,
                )
                .await
            }
        };

        // Initialize consensus
        let consensus = Consensus::new(
            blockchain_proxy.clone(),
            Arc::clone(&network),
            syncer_proxy,
            config.consensus.min_peers,
            syncer_tracker,
        );

        // Start network.
        network.listen_on(config.network.listen_addresses).await?;
        network.start_connecting().await;

        Ok(Client {
            inner: Arc::new(ClientInner {
                network,
                consensus: consensus.proxy(),
                blockchain: blockchain_proxy,
            }),
            consensus: Some(consensus),
        })
    }
}

/// Entry point for the Nimiq client API.
pub struct Client {
    inner: Arc<ClientInner>,
    consensus: Option<Consensus>,
}

impl Client {
    pub async fn from_config(config: ClientConfig) -> Result<Self, Error> {
        ClientInner::from_config(config).await
    }

    pub fn take_consensus(&mut self) -> Option<Consensus> {
        self.consensus.take()
    }

    /// Returns a reference to the *Consensus proxy*.
    pub fn consensus_proxy(&self) -> ConsensusProxy {
        self.inner.consensus.clone()
    }

    /// Returns a reference to the *Network* stack
    pub fn network(&self) -> Arc<Network> {
        Arc::clone(&self.inner.network)
    }

    /// Returns a reference to the blockchain
    pub fn blockchain(&self) -> BlockchainProxy {
        self.inner.blockchain.clone()
    }

    /// Returns the blockchain head
    pub fn blockchain_head(&self) -> Block {
        self.inner.blockchain.read().head().clone()
    }

    /// Returns the blockchain protocol version
    pub fn protocol_version(&self) -> u16 {
        self.inner.blockchain.read().protocol_version()
    }
}
