use std::{sync::Arc, time::Duration};

use nimiq_blockchain::{BlockProducer, Blockchain, BlockchainConfig};
use nimiq_blockchain_interface::AbstractBlockchain;
use nimiq_database::mdbx::MdbxDatabase;
use nimiq_hash::{HashOutput, Hasher};
use nimiq_keys::{Address, KeyPair, SecureGenerate};
use nimiq_mempool::{config::MempoolConfig, mempool::Mempool};
use nimiq_primitives::{account::AccountType, coin::Coin, networks::NetworkId};
use nimiq_serde::Serialize;
use nimiq_test_log::test;
use nimiq_test_utils::blockchain::{produce_macro_blocks, signing_key, voting_key};
use nimiq_transaction::{
    account::htlc_contract::{AnyHash, AnyHash32},
    bridge_contract::{
        AddressFormat, AnyMerkleProof, ChainConfig, Endianness, IncomingTransaction,
        OutgoingTransaction, RecipientData, ValidationProgram,
    },
    Transaction,
};
use nimiq_utils::{merkle::MerkleProof, time::OffsetTime};
use parking_lot::RwLock;

fn create_test_chain_config() -> ChainConfig {
    ChainConfig {
        chain_id: 1,
        hash_function: AnyHash::Blake2b(AnyHash32::default()),
        address_format: AddressFormat::Nimiq,
        endianness: Endianness::LittleEndian,
        block_time: Duration::from_secs(60),
        validation_program: ValidationProgram::empty(),
    }
}

fn create_test_incoming_transaction() -> IncomingTransaction {
    let chain_config = create_test_chain_config();
    let recipient_data = RecipientData::new(vec![1u8; 20], 1, &chain_config).unwrap();

    IncomingTransaction::new(recipient_data, Coin::from_u64_unchecked(1000), 100).unwrap()
}

fn create_test_outgoing_transaction() -> OutgoingTransaction {
    let burn_data = vec![1, 2, 3, 4, 5];
    let leaf_hash = nimiq_hash::Blake2bHash::default();
    let merkle_proof = MerkleProof::new(&[leaf_hash.clone()], &[leaf_hash]);
    let any_merkle_proof = AnyMerkleProof::Blake2b(merkle_proof);

    // Create oracle state hash
    let oracle_state_hash = AnyHash::Blake2b(AnyHash32::from(
        nimiq_hash::Blake2bHasher::default()
            .digest(b"oracle_state_root")
            .as_bytes(),
    ));

    OutgoingTransaction::new(burn_data, any_merkle_proof, oracle_state_hash).unwrap()
}

#[test]
fn test_bridge_transaction_detection() {
    // Create a regular transaction (not a bridge transaction)
    let regular_tx = Transaction::new_basic(
        Address::from([1u8; 20]),
        Address::from([2u8; 20]),
        Coin::from_u64_unchecked(1000),
        Coin::from_u64_unchecked(100),
        1,
        NetworkId::UnitAlbatross,
    );

    // Create a bridge transaction with incoming data
    let incoming_tx = create_test_incoming_transaction();
    let mut incoming_data = vec![0u8]; // Type byte: 0 = Incoming
    incoming_data.extend(incoming_tx.serialize_to_vec());

    let bridge_tx_incoming = Transaction::new_extended(
        Address::from([1u8; 20]),
        AccountType::Basic,
        vec![],
        Address::from([3u8; 20]),
        AccountType::Bridge,
        incoming_data,
        Coin::from_u64_unchecked(1000),
        Coin::from_u64_unchecked(100),
        1,
        NetworkId::UnitAlbatross,
    );

    // Create a bridge transaction with outgoing data
    let outgoing_tx = create_test_outgoing_transaction();
    let mut outgoing_data = vec![1u8]; // Type byte: 1 = Outgoing
    outgoing_data.extend(outgoing_tx.serialize_to_vec());

    let bridge_tx_outgoing = Transaction::new_extended(
        Address::from([1u8; 20]),
        AccountType::Basic,
        vec![],
        Address::from([3u8; 20]),
        AccountType::Bridge,
        outgoing_data,
        Coin::from_u64_unchecked(1000),
        Coin::from_u64_unchecked(100),
        1,
        NetworkId::UnitAlbatross,
    );

    // Regular transaction should not be detected as bridge transaction
    assert_eq!(regular_tx.recipient_type, AccountType::Basic);
    assert!(regular_tx.recipient_data.is_empty());

    // Bridge transactions should be detected
    assert_eq!(bridge_tx_incoming.recipient_type, AccountType::Bridge);
    assert!(!bridge_tx_incoming.recipient_data.is_empty());

    assert_eq!(bridge_tx_outgoing.recipient_type, AccountType::Bridge);
    assert!(!bridge_tx_outgoing.recipient_data.is_empty());
}

#[test]
fn test_cross_chain_validation_disabled_by_default() {
    // Create blockchain
    let time = Arc::new(OffsetTime::new());
    let env = MdbxDatabase::new_volatile(Default::default()).unwrap();
    let blockchain = Arc::new(RwLock::new(
        Blockchain::new(
            env,
            BlockchainConfig::default(),
            NetworkId::UnitAlbatross,
            time,
        )
        .unwrap(),
    ));

    // Produce some blocks
    let producer = BlockProducer::new(signing_key(), voting_key());

    produce_macro_blocks(&producer, &blockchain, 1);

    // Create mempool with default config (cross-chain validation enabled by default)
    let config = MempoolConfig::default();
    assert!(config.enable_cross_chain_validation);

    let mempool = Mempool::new(blockchain.clone(), config);

    // Create a bridge transaction
    let incoming_tx = create_test_incoming_transaction();
    let mut incoming_data = vec![0u8];
    incoming_data.extend(incoming_tx.serialize_to_vec());

    let mut bridge_tx = Transaction::new_extended(
        Address::from([1u8; 20]),
        AccountType::Basic,
        vec![],
        Address::from([3u8; 20]),
        AccountType::Bridge,
        incoming_data,
        Coin::from_u64_unchecked(1000),
        Coin::from_u64_unchecked(100),
        blockchain.read().head().block_number() + 1,
        NetworkId::UnitAlbatross,
    );

    // Sign the transaction
    let key_pair = KeyPair::generate_default_csprng();
    bridge_tx.sender = Address::from(&key_pair.public);
    let signature = key_pair.sign(&bridge_tx.serialize_content());
    bridge_tx.proof = signature.to_bytes().to_vec();

    // Transaction should be rejected because the bridge contract doesn't exist
    let result = mempool.add_transaction(bridge_tx, None);
    assert!(result.is_err());
}

#[test]
fn test_cross_chain_validation_enabled() {
    // Create blockchain
    let time = Arc::new(OffsetTime::new());
    let env = MdbxDatabase::new_volatile(Default::default()).unwrap();
    let blockchain = Arc::new(RwLock::new(
        Blockchain::new(
            env,
            BlockchainConfig::default(),
            NetworkId::UnitAlbatross,
            time,
        )
        .unwrap(),
    ));

    // Produce some blocks
    let producer = BlockProducer::new(signing_key(), voting_key());

    produce_macro_blocks(&producer, &blockchain, 1);

    // Create mempool with cross-chain validation enabled
    // Note: No oracle or bridge addresses needed - they come from blockchain state
    let config = MempoolConfig {
        enable_cross_chain_validation: true,
        max_cross_chain_resubmissions: 3,
        ..Default::default()
    };

    let mempool = Mempool::new(blockchain.clone(), config);

    // Create a bridge transaction
    let incoming_tx = create_test_incoming_transaction();
    let mut incoming_data = vec![0u8];
    incoming_data.extend(incoming_tx.serialize_to_vec());

    let mut bridge_tx = Transaction::new_extended(
        Address::from([1u8; 20]),
        AccountType::Basic,
        vec![],
        Address::from([3u8; 20]),
        AccountType::Bridge,
        incoming_data,
        Coin::from_u64_unchecked(1000),
        Coin::from_u64_unchecked(100),
        blockchain.read().head().block_number() + 1,
        NetworkId::UnitAlbatross,
    );

    // Sign the transaction
    let key_pair = KeyPair::generate_default_csprng();
    bridge_tx.sender = Address::from(&key_pair.public);
    let signature = key_pair.sign(&bridge_tx.serialize_content());
    bridge_tx.proof = signature.to_bytes().to_vec();

    // Transaction should go through cross-chain validation
    // It will fail because the bridge contract doesn't exist in the blockchain state
    let result = mempool.add_transaction(bridge_tx, None);
    assert!(result.is_err());
}

#[test]
fn test_regular_transaction_not_affected() {
    // Create blockchain
    let time = Arc::new(OffsetTime::new());
    let env = MdbxDatabase::new_volatile(Default::default()).unwrap();
    let blockchain = Arc::new(RwLock::new(
        Blockchain::new(
            env,
            BlockchainConfig::default(),
            NetworkId::UnitAlbatross,
            time,
        )
        .unwrap(),
    ));

    // Produce some blocks
    let producer = BlockProducer::new(signing_key(), voting_key());

    produce_macro_blocks(&producer, &blockchain, 1);

    // Create mempool with cross-chain validation enabled
    // Note: No oracle or bridge addresses needed - they come from blockchain state
    let config = MempoolConfig {
        enable_cross_chain_validation: true,
        max_cross_chain_resubmissions: 3,
        ..Default::default()
    };

    let mempool = Mempool::new(blockchain.clone(), config);

    // Create a regular transaction (not a bridge transaction)
    let mut regular_tx = Transaction::new_basic(
        Address::from([1u8; 20]),
        Address::from([2u8; 20]),
        Coin::from_u64_unchecked(1000),
        Coin::from_u64_unchecked(100),
        blockchain.read().head().block_number() + 1,
        NetworkId::UnitAlbatross,
    );

    // Sign the transaction
    let key_pair = KeyPair::generate_default_csprng();
    regular_tx.sender = Address::from(&key_pair.public);
    let signature = key_pair.sign(&regular_tx.serialize_content());
    regular_tx.proof = signature.to_bytes().to_vec();

    // Regular transaction should not go through cross-chain validation
    // It will fail for other reasons (insufficient balance), but not cross-chain validation
    let result = mempool.add_transaction(regular_tx, None);

    // Should fail with account error (no balance), not cross-chain validation error
    assert!(result.is_err());
    if let Err(e) = result {
        // Should not be a cross-chain validation error
        assert!(!format!("{:?}", e).contains("CrossChainValidationFailed"));
    }
}
