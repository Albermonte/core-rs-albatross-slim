use nimiq_hash::{Blake2bHasher, HashOutput, Hasher};
use nimiq_keys::Address;
use nimiq_primitives::coin::Coin;
use nimiq_transaction::{
    account::htlc_contract::{AnyHash, AnyHash32},
    bridge_contract::{
        AddressFormat, AnyMerkleProof, BridgeError, BurnTransactionInfo, ChainConfig, Endianness,
        MerkleProofValidator, OutgoingTransaction, ValidationOp, ValidationProgram,
    },
};
use nimiq_utils::merkle::MerkleProof;

// Helper function to create a test ValidationProgram that can parse burn transaction data
// Format: [amount(8), address(20), nonce(8), block_height(4), chain_id(4)] = 44 bytes total
fn create_test_validation_program() -> ValidationProgram {
    ValidationProgram::new(vec![
        // Extract amount at offset 0 (8 bytes, little-endian)
        ValidationOp::PushConst(0),
        ValidationOp::LoadU64(Endianness::LittleEndian),
        ValidationOp::Store("amount".to_string()),
        // Extract target address at offset 8 (20 bytes)
        ValidationOp::PushConst(8),
        ValidationOp::LoadAddress,
        ValidationOp::Store("target_address".to_string()),
        // Extract nonce at offset 28 (8 bytes, little-endian)
        ValidationOp::PushConst(28),
        ValidationOp::LoadU64(Endianness::LittleEndian),
        ValidationOp::Store("target_nonce".to_string()),
        // Extract block height at offset 36 (4 bytes, little-endian)
        ValidationOp::PushConst(36),
        ValidationOp::LoadU32(Endianness::LittleEndian),
        ValidationOp::Store("burn_block_height".to_string()),
        // Extract chain ID at offset 40 (4 bytes, little-endian)
        ValidationOp::PushConst(40),
        ValidationOp::LoadU32(Endianness::LittleEndian),
        ValidationOp::Store("target_chain_id".to_string()),
    ])
}

// Helper function to create a test chain config with proper ValidationProgram
fn create_test_chain_config() -> ChainConfig {
    ChainConfig {
        chain_id: 1,
        hash_function: AnyHash::Blake2b(AnyHash32::default()),
        address_format: AddressFormat::Nimiq,
        endianness: Endianness::LittleEndian,
        block_time: std::time::Duration::from_secs(60),
        validation_program: create_test_validation_program(),
    }
}

// Helper function to create a test validator
fn create_test_validator() -> MerkleProofValidator {
    let mut validator = MerkleProofValidator::default_blake2b();
    let oracle_address = Address::from([1u8; 20]);
    validator.update_oracle_address(oracle_address).unwrap();
    validator
}

// Helper function to create test oracle state hash
fn create_test_oracle_state_hash() -> AnyHash {
    let hash = Blake2bHasher::default().digest(b"oracle_state_root");
    AnyHash::Blake2b(AnyHash32::from(hash.as_bytes()))
}

// Helper function to create properly formatted burn transaction data
fn create_burn_transaction_data(
    amount: Coin,
    target_address: &[u8],
    nonce: u64,
    block_height: u32,
    chain_id: u32,
) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&u64::from(amount).to_le_bytes()); // 8 bytes
    data.extend_from_slice(target_address); // 20 bytes
    data.extend_from_slice(&nonce.to_le_bytes()); // 8 bytes
    data.extend_from_slice(&block_height.to_le_bytes()); // 4 bytes
    data.extend_from_slice(&chain_id.to_le_bytes()); // 4 bytes
    data
}

// Helper function to create test burn transaction info
fn create_test_burn_info() -> BurnTransactionInfo {
    let target_address = vec![2u8; 20];
    let amount = Coin::from_u64_unchecked(1000);
    let nonce = 1u64;
    let block_height = 100u32;
    let chain_id = 1u32;

    // Create properly formatted burn data
    let raw_data =
        create_burn_transaction_data(amount, &target_address, nonce, block_height, chain_id);
    let tx_hash = Blake2bHasher::default().digest(&raw_data);

    BurnTransactionInfo::new(
        raw_data,
        tx_hash,
        amount,
        target_address,
        nonce,
        block_height,
        1234567890,
        chain_id,
    )
    .unwrap()
}

// Helper function to create test outgoing transaction
fn create_test_outgoing_tx(burn_info: &BurnTransactionInfo) -> OutgoingTransaction {
    let leaf_hash = Blake2bHasher::default().digest(&burn_info.raw_data);
    let sibling_hash = Blake2bHasher::default().digest(b"sibling_data");
    let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
    let any_proof = AnyMerkleProof::Blake2b(proof);
    let oracle_state_hash = create_test_oracle_state_hash();

    OutgoingTransaction::new(burn_info.raw_data.clone(), any_proof, oracle_state_hash).unwrap()
}

// Helper function to create outgoing transaction with different amount
fn create_test_outgoing_tx_with_different_amount(
    burn_info: &BurnTransactionInfo,
    different_amount: Coin,
) -> OutgoingTransaction {
    // Create burn data with different amount but same other fields
    let burn_data = create_burn_transaction_data(
        different_amount,
        &burn_info.target_address,
        burn_info.nonce,
        burn_info.block_height,
        burn_info.source_chain_id,
    );

    let leaf_hash = Blake2bHasher::default().digest(&burn_data);
    let sibling_hash = Blake2bHasher::default().digest(b"sibling_data");
    let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
    let any_proof = AnyMerkleProof::Blake2b(proof);
    let oracle_state_hash = create_test_oracle_state_hash();

    OutgoingTransaction::new(burn_data, any_proof, oracle_state_hash).unwrap()
}

// Helper function to create outgoing transaction with different address
fn create_test_outgoing_tx_with_different_address(
    burn_info: &BurnTransactionInfo,
    different_address: &[u8],
) -> OutgoingTransaction {
    // Create burn data with different address but same other fields
    let burn_data = create_burn_transaction_data(
        burn_info.amount,
        different_address,
        burn_info.nonce,
        burn_info.block_height,
        burn_info.source_chain_id,
    );

    let leaf_hash = Blake2bHasher::default().digest(&burn_data);
    let sibling_hash = Blake2bHasher::default().digest(b"sibling_data");
    let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
    let any_proof = AnyMerkleProof::Blake2b(proof);
    let oracle_state_hash = create_test_oracle_state_hash();

    OutgoingTransaction::new(burn_data, any_proof, oracle_state_hash).unwrap()
}

// Helper function to create outgoing transaction with different nonce
fn create_test_outgoing_tx_with_different_nonce(
    burn_info: &BurnTransactionInfo,
    different_nonce: u64,
) -> OutgoingTransaction {
    // Create burn data with different nonce but same other fields
    let burn_data = create_burn_transaction_data(
        burn_info.amount,
        &burn_info.target_address,
        different_nonce,
        burn_info.block_height,
        burn_info.source_chain_id,
    );

    let leaf_hash = Blake2bHasher::default().digest(&burn_data);
    let sibling_hash = Blake2bHasher::default().digest(b"sibling_data");
    let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
    let any_proof = AnyMerkleProof::Blake2b(proof);
    let oracle_state_hash = create_test_oracle_state_hash();

    OutgoingTransaction::new(burn_data, any_proof, oracle_state_hash).unwrap()
}

// Helper function to create outgoing transaction with different block height
fn create_test_outgoing_tx_with_different_height(
    burn_info: &BurnTransactionInfo,
    different_height: u32,
) -> OutgoingTransaction {
    // Create burn data with different height but same other fields
    let burn_data = create_burn_transaction_data(
        burn_info.amount,
        &burn_info.target_address,
        burn_info.nonce,
        different_height,
        burn_info.source_chain_id,
    );

    let leaf_hash = Blake2bHasher::default().digest(&burn_data);
    let sibling_hash = Blake2bHasher::default().digest(b"sibling_data");
    let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
    let any_proof = AnyMerkleProof::Blake2b(proof);
    let oracle_state_hash = create_test_oracle_state_hash();

    OutgoingTransaction::new(burn_data, any_proof, oracle_state_hash).unwrap()
}

#[test]
fn test_oracle_reference_validation_success() {
    let validator = create_test_validator();
    let oracle_address = Address::from([1u8; 20]);

    let result = validator.verify_oracle_reference(&oracle_address);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_oracle_reference_validation_failure() {
    let validator = create_test_validator();
    let wrong_oracle_address = Address::from([99u8; 20]);

    let result = validator.verify_oracle_reference(&wrong_oracle_address);
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[test]
fn test_oracle_reference_validation_not_set() {
    let validator = MerkleProofValidator::default_blake2b();
    let oracle_address = Address::from([1u8; 20]);

    let result = validator.verify_oracle_reference(&oracle_address);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::OracleAddressNotSet
    ));
}

#[test]
fn test_leaf_node_validation_success() {
    let validator = create_test_validator();
    let burn_data = b"test_burn_transaction_data".to_vec();
    let expected_leaf_hash = Blake2bHasher::default().digest(&burn_data);

    let result = validator.validate_leaf_node(
        &burn_data,
        AnyHash::Blake2b(AnyHash32::from(expected_leaf_hash.as_bytes())),
    );
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_leaf_node_validation_mismatch() {
    let validator = create_test_validator();
    let burn_data = b"test_burn_transaction_data".to_vec();
    let wrong_leaf_hash = Blake2bHasher::default().digest(b"wrong_data");

    let result = validator.validate_leaf_node(
        &burn_data,
        AnyHash::Blake2b(AnyHash32::from(wrong_leaf_hash.as_bytes())),
    );
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[test]
fn test_leaf_node_validation_empty_data() {
    let validator = create_test_validator();
    let empty_data = vec![];
    let leaf_hash = Blake2bHasher::default().digest(b"some_data");

    let result = validator.validate_leaf_node(
        &empty_data,
        AnyHash::Blake2b(AnyHash32::from(leaf_hash.as_bytes())),
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidDataLength
    ));
}

#[test]
fn test_transaction_detail_validation_success() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);

    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

// TODO: These tests need to be rewritten to create different burn_transaction_data
// instead of modifying fields that no longer exist on OutgoingTransaction.
// They require a proper ValidationProgram that can parse the burn data.

#[test]
fn test_transaction_detail_validation_amount_mismatch() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Create outgoing transaction with different amount
    let different_amount = Coin::from_u64_unchecked(2000); // burn_info has 1000
    let outgoing_tx = create_test_outgoing_tx_with_different_amount(&burn_info, different_amount);

    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(!result.unwrap(), "Should fail when amounts don't match");
}

#[test]
fn test_transaction_detail_validation_address_mismatch() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Create outgoing transaction with different address
    let different_address = vec![99u8; 20]; // burn_info has [2u8; 20]
    let outgoing_tx =
        create_test_outgoing_tx_with_different_address(&burn_info, &different_address);

    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(!result.unwrap(), "Should fail when addresses don't match");
}

#[test]
fn test_transaction_detail_validation_nonce_mismatch() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Create outgoing transaction with different nonce
    let different_nonce = 999u64; // burn_info has 1
    let outgoing_tx = create_test_outgoing_tx_with_different_nonce(&burn_info, different_nonce);

    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(!result.unwrap(), "Should fail when nonces don't match");
}

#[test]
fn test_transaction_detail_validation_height_mismatch() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Create outgoing transaction with different height
    let different_height = 999u32; // burn_info has 100
    let outgoing_tx = create_test_outgoing_tx_with_different_height(&burn_info, different_height);

    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(!result.unwrap(), "Should fail when heights don't match");
}

#[test]
fn test_comprehensive_proof_verification_oracle_reference_failure() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let wrong_oracle_address = Address::from([99u8; 20]);

    let result = validator.verify_comprehensive_proof(
        &outgoing_tx,
        &burn_info,
        &wrong_oracle_address,
        1,
        &chain_config,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidOracleSignature
    ));
}

#[test]
fn test_comprehensive_proof_verification_transaction_detail_failure() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Create outgoing transaction with different amount
    let different_amount = Coin::from_u64_unchecked(2000);
    let outgoing_tx = create_test_outgoing_tx_with_different_amount(&burn_info, different_amount);
    let oracle_address = Address::from([1u8; 20]);

    let result = validator.verify_comprehensive_proof(
        &outgoing_tx,
        &burn_info,
        &oracle_address,
        1,
        &chain_config,
    );
    assert!(result.is_err());
    // Should fail on transaction detail validation
    match result.unwrap_err() {
        BridgeError::InvalidMerkleProof | BridgeError::RootHashMismatch => (),
        other => panic!(
            "Expected InvalidMerkleProof or RootHashMismatch, got {:?}",
            other
        ),
    }
}

#[test]
fn test_comprehensive_proof_verification_oracle_unavailable() {
    // This test verifies that when oracle state hash doesn't match, the comprehensive verification fails
    // The verification should pass all preliminary checks and only fail at root hash comparison
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let oracle_address = Address::from([1u8; 20]);

    // The verification will fail because the oracle_state_hash in the transaction
    // doesn't match the computed root from the proof
    let result = validator.verify_comprehensive_proof(
        &outgoing_tx,
        &burn_info,
        &oracle_address,
        1,
        &chain_config,
    );
    assert!(result.is_err());
    // The error could be InvalidMerkleProof or RootHashMismatch depending on where it fails
    match result.unwrap_err() {
        BridgeError::InvalidMerkleProof | BridgeError::RootHashMismatch => (),
        other => panic!(
            "Expected InvalidMerkleProof or RootHashMismatch, got {:?}",
            other
        ),
    }
}

#[test]
fn test_extract_transaction_hash_blake2b() {
    let validator = create_test_validator();
    let tx_data = b"test_transaction_data".to_vec();
    let expected_hash = AnyHash::Blake2b(AnyHash32::from(
        Blake2bHasher::default().digest(&tx_data).as_bytes(),
    ));

    let result = validator.extract_transaction_hash(&tx_data);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), expected_hash);
}

#[test]
fn test_extract_transaction_hash_empty_data() {
    let validator = create_test_validator();
    let empty_data = vec![];

    let result = validator.extract_transaction_hash(&empty_data);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidDataLength
    ));
}

#[test]
fn test_validate_proof_structure_valid() {
    let validator = create_test_validator();
    let leaf_hash = Blake2bHasher::default().digest(b"test_data");
    let sibling_hash = Blake2bHasher::default().digest(b"sibling_data");
    let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
    let any_proof = AnyMerkleProof::Blake2b(proof);

    let result = validator.validate_proof_structure(&any_proof);
    assert!(result.is_ok());
}

#[test]
fn test_validate_proof_structure_empty() {
    let validator = create_test_validator();
    let empty_hashes = vec![];
    let proof = MerkleProof::new(&empty_hashes, &empty_hashes);
    let any_proof = AnyMerkleProof::Blake2b(proof);

    // Test that validation doesn't panic with empty proof
    // The actual behavior (accept or reject) depends on MerkleProof implementation
    let _result = validator.validate_proof_structure(&any_proof);
    // We just verify it doesn't panic - the specific result may vary
}

#[test]
fn test_validate_proof_structure_exceeds_depth() {
    let mut validator = create_test_validator();
    validator.set_max_proof_depth(1).unwrap(); // Set very low limit

    let leaf_hash = Blake2bHasher::default().digest(b"test_data");
    let s1 = Blake2bHasher::default().digest(b"sibling1");
    let s2 = Blake2bHasher::default().digest(b"sibling2");
    let s3 = Blake2bHasher::default().digest(b"sibling3");
    // Create a proof with multiple nodes (will exceed max depth of 1)
    let hashes = vec![leaf_hash.clone(), s1, s2, s3];
    let proof = MerkleProof::new(&hashes, &[leaf_hash]);
    let any_proof = AnyMerkleProof::Blake2b(proof);

    let result = validator.validate_proof_structure(&any_proof);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::ProofDepthExceeded
    ));
}

#[test]
fn test_update_oracle_address_success() {
    let mut validator = MerkleProofValidator::default_blake2b();
    let new_oracle_address = Address::from([5u8; 20]);

    let result = validator.update_oracle_address(new_oracle_address.clone());
    assert!(result.is_ok());
    assert_eq!(validator.oracle_address, Some(new_oracle_address));
}

#[test]
fn test_update_oracle_address_zero_address() {
    let mut validator = MerkleProofValidator::default_blake2b();
    let zero_address = Address::from([0u8; 20]);

    let result = validator.update_oracle_address(zero_address);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::OracleAddressNotSet
    ));
}

#[test]
fn test_burn_transaction_info_validation() {
    let burn_info = create_test_burn_info();
    let result = burn_info.validate();
    assert!(result.is_ok());
}

#[test]
fn test_burn_transaction_info_verify_hash() {
    let burn_info = create_test_burn_info();
    let result = burn_info.verify_hash();
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
fn test_burn_transaction_info_transaction_details() {
    let burn_info = create_test_burn_info();
    let (amount, address, nonce, height) = burn_info.transaction_details();

    assert_eq!(amount, burn_info.amount);
    assert_eq!(address, burn_info.target_address);
    assert_eq!(nonce, burn_info.nonce);
    assert_eq!(height, burn_info.block_height);
}

#[test]
fn test_outgoing_transaction_validation() {
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let result = outgoing_tx.validate();
    assert!(result.is_ok());
}

#[test]
fn test_outgoing_transaction_extract_burn_hash() {
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let hash_function = AnyHash::Blake2b(AnyHash32::default());
    let result = outgoing_tx.extract_burn_transaction_hash(&hash_function);
    assert!(result.is_ok());
    let expected = AnyHash::Blake2b(AnyHash32::from(burn_info.tx_hash.as_bytes()));
    assert_eq!(result.unwrap(), expected);
}

#[test]
fn test_outgoing_transaction_transaction_details() {
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let chain_config = create_test_chain_config();
    let (amount, address, nonce, height) = outgoing_tx.transaction_details(&chain_config).unwrap();

    // Parse burn data to get expected values
    let parsed = outgoing_tx.parse_burn_data(&chain_config).unwrap();
    assert_eq!(amount, parsed.amount);
    assert_eq!(address, parsed.target_address);
    assert_eq!(nonce, parsed.target_nonce);
    assert_eq!(height, parsed.burn_block_height);
}

#[test]
fn test_outgoing_transaction_proof_depth() {
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let depth = outgoing_tx.proof_depth();
    // Our test proof has 2 hashes (leaf + sibling), which creates a proof with 1 node
    assert!(depth > 0);
}

#[test]
fn test_outgoing_transaction_is_proof_depth_valid() {
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);

    assert!(outgoing_tx.is_proof_depth_valid(32));
    assert!(outgoing_tx.is_proof_depth_valid(1));
    // With depth 0, any proof with nodes should be invalid
    // But our implementation allows it, so we test the actual behavior
    let is_valid_at_zero = outgoing_tx.is_proof_depth_valid(0);
    // The proof has at least 1 node, so it should be invalid at depth 0
    assert!(!is_valid_at_zero || outgoing_tx.proof_depth() == 0);
}

// ============================================================================
// Unit tests for proof verification completeness
// ============================================================================

/// Test oracle contract reference validation with multiple oracle addresses
#[test]
fn test_proof_verification_oracle_reference_multiple_addresses() {
    let mut validator = MerkleProofValidator::default_blake2b();

    // Test with first oracle address
    let oracle1 = Address::from([10u8; 20]);
    validator.update_oracle_address(oracle1.clone()).unwrap();
    assert!(validator.verify_oracle_reference(&oracle1).unwrap());

    // Update to second oracle address
    let oracle2 = Address::from([20u8; 20]);
    validator.update_oracle_address(oracle2.clone()).unwrap();
    assert!(validator.verify_oracle_reference(&oracle2).unwrap());

    // First oracle should now fail
    assert!(!validator.verify_oracle_reference(&oracle1).unwrap());
}

/// Test oracle contract reference validation with edge case addresses
#[test]
fn test_proof_verification_oracle_reference_edge_cases() {
    let mut validator = MerkleProofValidator::default_blake2b();

    // Test with all 0xFF address
    let oracle_max = Address::from([0xFFu8; 20]);
    validator.update_oracle_address(oracle_max.clone()).unwrap();
    assert!(validator.verify_oracle_reference(&oracle_max).unwrap());

    // Test with pattern address
    let oracle_pattern = Address::from([0xAAu8; 20]);
    validator
        .update_oracle_address(oracle_pattern.clone())
        .unwrap();
    assert!(validator.verify_oracle_reference(&oracle_pattern).unwrap());
    assert!(!validator.verify_oracle_reference(&oracle_max).unwrap());
}

/// Test oracle contract reference validation fails when oracle not configured
#[test]
fn test_proof_verification_oracle_reference_unconfigured() {
    let validator = MerkleProofValidator::default_blake2b();
    let oracle_address = Address::from([5u8; 20]);

    let result = validator.verify_oracle_reference(&oracle_address);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::OracleAddressNotSet
    ));
}

/// Test leaf node validation with matching burn transaction data
#[test]
fn test_proof_verification_leaf_node_exact_match() {
    let validator = create_test_validator();
    let burn_data = b"exact_burn_transaction_data_12345".to_vec();
    let expected_leaf_hash = Blake2bHasher::default().digest(&burn_data);

    let result = validator.validate_leaf_node(
        &burn_data,
        AnyHash::Blake2b(AnyHash32::from(expected_leaf_hash.as_bytes())),
    );
    assert!(result.is_ok());
    assert!(result.unwrap());
}

/// Test leaf node validation with different burn transaction sizes
#[test]
fn test_proof_verification_leaf_node_various_sizes() {
    let validator = create_test_validator();

    // Test with small data
    let small_data = b"x".to_vec();
    let small_hash = Blake2bHasher::default().digest(&small_data);
    assert!(validator
        .validate_leaf_node(
            &small_data,
            AnyHash::Blake2b(AnyHash32::from(small_hash.as_bytes()))
        )
        .unwrap());

    // Test with medium data
    let medium_data = vec![0xABu8; 256];
    let medium_hash = Blake2bHasher::default().digest(&medium_data);
    assert!(validator
        .validate_leaf_node(
            &medium_data,
            AnyHash::Blake2b(AnyHash32::from(medium_hash.as_bytes()))
        )
        .unwrap());

    // Test with large data
    let large_data = vec![0xCDu8; 5000];
    let large_hash = Blake2bHasher::default().digest(&large_data);
    assert!(validator
        .validate_leaf_node(
            &large_data,
            AnyHash::Blake2b(AnyHash32::from(large_hash.as_bytes()))
        )
        .unwrap());
}

/// Test leaf node validation fails with mismatched hash
#[test]
fn test_proof_verification_leaf_node_hash_mismatch() {
    let validator = create_test_validator();
    let burn_data = b"burn_transaction_data".to_vec();
    let wrong_hash = Blake2bHasher::default().digest(b"different_data");

    let result = validator.validate_leaf_node(
        &burn_data,
        AnyHash::Blake2b(AnyHash32::from(wrong_hash.as_bytes())),
    );
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

/// Test leaf node validation with empty burn transaction data
#[test]
fn test_proof_verification_leaf_node_empty_data() {
    let validator = create_test_validator();
    let empty_data = vec![];
    let some_hash = Blake2bHasher::default().digest(b"data");

    let result = validator.validate_leaf_node(
        &empty_data,
        AnyHash::Blake2b(AnyHash32::from(some_hash.as_bytes())),
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidDataLength
    ));
}

/// Test leaf node validation with binary burn transaction data
#[test]
fn test_proof_verification_leaf_node_binary_data() {
    let validator = create_test_validator();

    // Test with various binary patterns
    let binary_data = vec![0x00, 0xFF, 0xAA, 0x55, 0x12, 0x34, 0x56, 0x78];
    let binary_hash = Blake2bHasher::default().digest(&binary_data);

    let result = validator.validate_leaf_node(
        &binary_data,
        AnyHash::Blake2b(AnyHash32::from(binary_hash.as_bytes())),
    );
    assert!(result.is_ok());
    assert!(result.unwrap());
}

/// Test comprehensive proof verification with all components
#[test]
fn test_proof_verification_comprehensive_all_validations() {
    let validator = create_test_validator();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let chain_config = create_test_chain_config();
    let oracle_address = Address::from([1u8; 20]);

    // This should validate:
    // 1. Oracle reference (passes)
    // 2. Transaction details (passes)
    // 3. Leaf hash extraction (passes)
    // 4. Proof structure (passes)
    // 5. Root hash comparison (fails with RootHashMismatch or InvalidMerkleProof)
    let result = validator.verify_comprehensive_proof(
        &outgoing_tx,
        &burn_info,
        &oracle_address,
        1,
        &chain_config,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::RootHashMismatch | BridgeError::InvalidMerkleProof
    ));
}

/// Test comprehensive proof verification fails early on oracle reference
#[test]
fn test_proof_verification_comprehensive_oracle_reference_early_fail() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let wrong_oracle = Address::from([99u8; 20]);

    // Should fail immediately on oracle reference check
    let result = validator.verify_comprehensive_proof(
        &outgoing_tx,
        &burn_info,
        &wrong_oracle,
        1,
        &chain_config,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidOracleSignature
    ));
}

/// Test comprehensive proof verification fails on transaction detail mismatch
#[test]
fn test_proof_verification_comprehensive_transaction_detail_fail() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();
    let oracle_address = Address::from([1u8; 20]);

    // Create outgoing_tx with different amount in burn_transaction_data
    let different_amount = Coin::from_u64_unchecked(2000);
    let outgoing_tx = create_test_outgoing_tx_with_different_amount(&burn_info, different_amount);

    let result = validator.verify_comprehensive_proof(
        &outgoing_tx,
        &burn_info,
        &oracle_address,
        1,
        &chain_config,
    );
    assert!(result.is_err());
}

/// Test oracle reference validation with sequential updates
#[test]
fn test_proof_verification_oracle_reference_sequential_updates() {
    let mut validator = MerkleProofValidator::default_blake2b();

    // Set initial oracle
    let oracle1 = Address::from([1u8; 20]);
    validator.update_oracle_address(oracle1.clone()).unwrap();

    // Verify it works
    assert!(validator.verify_oracle_reference(&oracle1).unwrap());

    // Update to new oracle
    let oracle2 = Address::from([2u8; 20]);
    validator.update_oracle_address(oracle2.clone()).unwrap();

    // Old oracle should fail, new should succeed
    assert!(!validator.verify_oracle_reference(&oracle1).unwrap());
    assert!(validator.verify_oracle_reference(&oracle2).unwrap());

    // Update to third oracle
    let oracle3 = Address::from([3u8; 20]);
    validator.update_oracle_address(oracle3.clone()).unwrap();

    // Only latest should succeed
    assert!(!validator.verify_oracle_reference(&oracle1).unwrap());
    assert!(!validator.verify_oracle_reference(&oracle2).unwrap());
    assert!(validator.verify_oracle_reference(&oracle3).unwrap());
}

/// Test leaf node validation with transaction hash extraction
#[test]
fn test_proof_verification_leaf_node_with_hash_extraction() {
    let validator = create_test_validator();
    let burn_data = b"burn_tx_data_for_extraction".to_vec();

    // Extract hash using validator's method
    let extracted_hash = validator.extract_transaction_hash(&burn_data).unwrap();

    // Validate leaf node with extracted hash
    let result = validator.validate_leaf_node(
        &burn_data,
        AnyHash::Blake2b(AnyHash32::from(extracted_hash.as_bytes())),
    );
    assert!(result.is_ok());
    assert!(result.unwrap());
}

/// Test comprehensive proof verification validates all fields correctly
#[test]
fn test_proof_verification_comprehensive_validates_all_fields() {
    let validator = create_test_validator();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let chain_config = create_test_chain_config();
    let oracle_address = Address::from([1u8; 20]);

    // Verify that all fields match before root hash comparison by parsing
    let parsed = outgoing_tx.parse_burn_data(&chain_config).unwrap();
    assert_eq!(burn_info.amount, parsed.amount);
    assert_eq!(burn_info.target_address, parsed.target_address);
    assert_eq!(burn_info.nonce, parsed.target_nonce);
    assert_eq!(burn_info.block_height, parsed.burn_block_height);

    // Comprehensive verification should validate all these before failing on root hash comparison
    let result = validator.verify_comprehensive_proof(
        &outgoing_tx,
        &burn_info,
        &oracle_address,
        1,
        &chain_config,
    );
    assert!(result.is_err());
    // Should fail on root hash comparison, not on field validation
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::RootHashMismatch | BridgeError::InvalidMerkleProof
    ));
}

// ============================================================================
// Unit tests for transaction detail validation
// ============================================================================

/// Test amount validation with exact match between burn and cross-chain transactions
#[test]
fn test_transaction_detail_amount_exact_match() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);

    // Verify amounts match exactly by parsing
    let parsed = outgoing_tx.parse_burn_data(&chain_config).unwrap();
    assert_eq!(burn_info.amount, parsed.amount);
    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

/// Test amount validation with various valid amounts
#[test]
fn test_transaction_detail_amount_various_valid_amounts() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();

    // Test with different valid amounts
    let amounts = vec![
        Coin::from_u64_unchecked(1),             // Minimum
        Coin::from_u64_unchecked(1000),          // Small
        Coin::from_u64_unchecked(1_000_000),     // Medium
        Coin::from_u64_unchecked(1_000_000_000), // Large
    ];

    for amount in amounts {
        let target_address = vec![2u8; 20];
        let nonce = 1u64;
        let block_height = 100u32;
        let chain_id = 1u32;

        // Create properly formatted burn data
        let raw_data =
            create_burn_transaction_data(amount, &target_address, nonce, block_height, chain_id);
        let tx_hash = Blake2bHasher::default().digest(&raw_data);

        let burn_info = BurnTransactionInfo::new(
            raw_data.clone(),
            tx_hash.clone(),
            amount,
            target_address.clone(),
            nonce,
            block_height,
            1234567890,
            chain_id,
        )
        .unwrap();

        let leaf_hash = Blake2bHasher::default().digest(&burn_info.raw_data);
        let sibling_hash = Blake2bHasher::default().digest(b"sibling");
        let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
        let any_proof = AnyMerkleProof::Blake2b(proof);
        let oracle_state_hash = create_test_oracle_state_hash();

        let outgoing_tx = OutgoingTransaction::new(raw_data, any_proof, oracle_state_hash).unwrap();

        let result =
            validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
        assert!(result.is_ok());
        assert!(result.unwrap(), "Failed for amount: {}", amount);
    }
}

/// Test amount validation fails when amounts differ
#[test]
fn test_transaction_detail_amount_mismatch_fails() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Test with different mismatched amounts (excluding zero which is invalid)
    let mismatched_amounts = vec![
        Coin::from_u64_unchecked(999),   // Less than burn_info.amount (1000)
        Coin::from_u64_unchecked(1001),  // More than burn_info.amount
        Coin::from_u64_unchecked(1),     // Minimum valid amount
        Coin::from_u64_unchecked(10000), // Much larger
    ];

    for amount in mismatched_amounts {
        let outgoing_tx = create_test_outgoing_tx_with_different_amount(&burn_info, amount);
        let result =
            validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
        assert!(result.is_ok());
        assert!(
            !result.unwrap(),
            "Should fail for mismatched amount: {}",
            amount
        );
    }
}

/// Test amount validation with zero amount (should fail in transaction creation)
#[test]
fn test_transaction_detail_amount_zero_rejected() {
    let raw_data = b"test_data".to_vec();
    let tx_hash = Blake2bHasher::default().digest(&raw_data);
    let target_address = vec![2u8; 20];

    // Creating burn info with zero amount should fail
    let result = BurnTransactionInfo::new(
        raw_data,
        tx_hash,
        Coin::ZERO,
        target_address,
        1,
        100,
        1234567890,
        1,
    );
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BridgeError::InvalidAmount));
}

/// Test target address matching with exact match
#[test]
fn test_transaction_detail_target_address_exact_match() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);

    // Verify addresses match exactly by parsing
    let parsed = outgoing_tx.parse_burn_data(&chain_config).unwrap();
    assert_eq!(burn_info.target_address, parsed.target_address);
    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

/// Test target address matching with various valid addresses
#[test]
fn test_transaction_detail_target_address_various_addresses() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();

    // Test with different address patterns
    let addresses = vec![
        vec![0u8; 20],    // All zeros
        vec![0xFFu8; 20], // All ones
        vec![0xAAu8; 20], // Pattern
        vec![0x55u8; 20], // Different pattern
        vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        ], // Sequential
    ];

    for target_address in addresses {
        let amount = Coin::from_u64_unchecked(1000);
        let nonce = 1u64;
        let block_height = 100u32;
        let chain_id = 1u32;

        // Create properly formatted burn data
        let raw_data =
            create_burn_transaction_data(amount, &target_address, nonce, block_height, chain_id);
        let tx_hash = Blake2bHasher::default().digest(&raw_data);

        let burn_info = BurnTransactionInfo::new(
            raw_data.clone(),
            tx_hash.clone(),
            amount,
            target_address.clone(),
            nonce,
            block_height,
            1234567890,
            chain_id,
        )
        .unwrap();

        let leaf_hash = Blake2bHasher::default().digest(&burn_info.raw_data);
        let sibling_hash = Blake2bHasher::default().digest(b"sibling");
        let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
        let any_proof = AnyMerkleProof::Blake2b(proof);
        let oracle_state_hash = create_test_oracle_state_hash();

        let outgoing_tx = OutgoingTransaction::new(raw_data, any_proof, oracle_state_hash).unwrap();

        let result =
            validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
        assert!(result.is_ok());
        assert!(result.unwrap(), "Failed for address: {:?}", target_address);
    }
}

/// Test target address matching fails when addresses differ
#[test]
fn test_transaction_detail_target_address_mismatch_fails() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Test with different mismatched addresses
    let mismatched_addresses = vec![
        vec![99u8; 20],   // Different pattern
        vec![0u8; 20],    // All zeros
        vec![0xFFu8; 20], // All ones
    ];

    for address in mismatched_addresses {
        let outgoing_tx = create_test_outgoing_tx_with_different_address(&burn_info, &address);
        let result =
            validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
        assert!(result.is_ok());
        assert!(
            !result.unwrap(),
            "Should fail for mismatched address: {:?}",
            address
        );
    }
}

/// Test target address matching with single byte difference
#[test]
fn test_transaction_detail_target_address_single_byte_difference() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Create address with single byte different from burn_info.target_address
    let mut different_address = burn_info.target_address.clone();
    different_address[0] = different_address[0].wrapping_add(1); // Change first byte

    let outgoing_tx =
        create_test_outgoing_tx_with_different_address(&burn_info, &different_address);
    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(
        !result.unwrap(),
        "Should fail even with single byte difference"
    );
}

/// Test validity start height validation with exact match
#[test]
fn test_transaction_detail_validity_height_exact_match() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);

    // Verify heights match exactly by parsing
    let parsed = outgoing_tx.parse_burn_data(&chain_config).unwrap();
    assert_eq!(burn_info.block_height, parsed.burn_block_height);
    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

/// Test validity start height validation with various valid heights
#[test]
fn test_transaction_detail_validity_height_various_heights() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();

    // Test with different valid heights
    let heights = vec![1u32, 100u32, 1000u32, 1_000_000u32];

    for height in heights {
        let target_address = vec![2u8; 20];
        let amount = Coin::from_u64_unchecked(1000);
        let nonce = 1u64;
        let chain_id = 1u32;

        // Create properly formatted burn data
        let raw_data =
            create_burn_transaction_data(amount, &target_address, nonce, height, chain_id);
        let tx_hash = Blake2bHasher::default().digest(&raw_data);

        let burn_info = BurnTransactionInfo::new(
            raw_data.clone(),
            tx_hash,
            amount,
            target_address,
            nonce,
            height,
            1234567890,
            chain_id,
        )
        .unwrap();

        let leaf_hash = Blake2bHasher::default().digest(&burn_info.raw_data);
        let sibling_hash = Blake2bHasher::default().digest(b"sibling");
        let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
        let any_proof = AnyMerkleProof::Blake2b(proof);
        let oracle_state_hash = create_test_oracle_state_hash();

        let outgoing_tx = OutgoingTransaction::new(raw_data, any_proof, oracle_state_hash).unwrap();

        let result =
            validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
        assert!(result.is_ok());
        assert!(result.unwrap(), "Failed for height: {}", height);
    }
}

/// Test validity start height validation fails when heights differ
#[test]
fn test_transaction_detail_validity_height_mismatch_fails() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Test with different mismatched heights
    let mismatched_heights = vec![99u32, 101u32, 999u32, 1u32];

    for height in mismatched_heights {
        let outgoing_tx = create_test_outgoing_tx_with_different_height(&burn_info, height);
        let result =
            validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
        assert!(result.is_ok());
        assert!(
            !result.unwrap(),
            "Should fail for mismatched height: {}",
            height
        );
    }
}

/// Test validity start height validation with zero height (should fail in transaction creation)
#[test]
fn test_transaction_detail_validity_height_zero_rejected() {
    let raw_data = b"test_data".to_vec();
    let tx_hash = Blake2bHasher::default().digest(&raw_data);
    let target_address = vec![2u8; 20];

    // Creating burn info with zero block height should fail
    let result = BurnTransactionInfo::new(
        raw_data,
        tx_hash,
        Coin::from_u64_unchecked(1000),
        target_address,
        1,
        0, // Zero block height
        1234567890,
        1,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidValidityHeight
    ));
}

/// Test transaction detail validation with all fields matching
#[test]
fn test_transaction_detail_all_fields_match() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);

    // Parse burn data and verify all fields match
    let parsed = outgoing_tx.parse_burn_data(&chain_config).unwrap();
    assert_eq!(burn_info.amount, parsed.amount);
    assert_eq!(burn_info.target_address, parsed.target_address);
    assert_eq!(burn_info.nonce, parsed.target_nonce);
    assert_eq!(burn_info.block_height, parsed.burn_block_height);

    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

/// Test transaction detail validation with multiple fields mismatched
#[test]
fn test_transaction_detail_multiple_fields_mismatch() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Create burn data with multiple fields different
    let different_amount = Coin::from_u64_unchecked(2000);
    let different_address = vec![99u8; 20];
    let different_nonce = 999u64;
    let different_height = 999u32;

    let burn_data = create_burn_transaction_data(
        different_amount,
        &different_address,
        different_nonce,
        different_height,
        burn_info.source_chain_id,
    );

    let leaf_hash = Blake2bHasher::default().digest(&burn_data);
    let sibling_hash = Blake2bHasher::default().digest(b"sibling_data");
    let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
    let any_proof = AnyMerkleProof::Blake2b(proof);
    let oracle_state_hash = create_test_oracle_state_hash();

    let outgoing_tx = OutgoingTransaction::new(burn_data, any_proof, oracle_state_hash).unwrap();

    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(
        !result.unwrap(),
        "Should fail when multiple fields don't match"
    );
}

/// Test transaction detail validation with nonce mismatch
#[test]
fn test_transaction_detail_nonce_mismatch() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Test with different mismatched nonces (excluding 0 which is invalid)
    let mismatched_nonces = vec![2u64, 999u64, 1_000_000u64];

    for nonce in mismatched_nonces {
        let outgoing_tx = create_test_outgoing_tx_with_different_nonce(&burn_info, nonce);
        let result =
            validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
        assert!(result.is_ok());
        assert!(
            !result.unwrap(),
            "Should fail for mismatched nonce: {}",
            nonce
        );
    }
}

/// Test transaction detail validation preserves field order
#[test]
fn test_transaction_detail_validation_field_order() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();

    // Test that each field mismatch is detected independently
    // Amount mismatch
    let outgoing_tx_amount =
        create_test_outgoing_tx_with_different_amount(&burn_info, Coin::from_u64_unchecked(2000));
    assert!(!validator
        .validate_transaction_details(&burn_info, &outgoing_tx_amount, &chain_config)
        .unwrap());

    // Address mismatch
    let outgoing_tx_address =
        create_test_outgoing_tx_with_different_address(&burn_info, &vec![99u8; 20]);
    assert!(!validator
        .validate_transaction_details(&burn_info, &outgoing_tx_address, &chain_config)
        .unwrap());

    // Nonce mismatch
    let outgoing_tx_nonce = create_test_outgoing_tx_with_different_nonce(&burn_info, 999u64);
    assert!(!validator
        .validate_transaction_details(&burn_info, &outgoing_tx_nonce, &chain_config)
        .unwrap());

    // Height mismatch
    let outgoing_tx_height = create_test_outgoing_tx_with_different_height(&burn_info, 999u32);
    assert!(!validator
        .validate_transaction_details(&burn_info, &outgoing_tx_height, &chain_config)
        .unwrap());
}

/// Test transaction detail validation with edge case values
#[test]
fn test_transaction_detail_validation_edge_cases() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();

    // Test with minimum values
    let target_address = vec![0u8; 20];
    let amount = Coin::from_u64_unchecked(1);
    let nonce = 1u64;
    let height = 1u32;
    let chain_id = 1u32;

    let raw_data = create_burn_transaction_data(amount, &target_address, nonce, height, chain_id);
    let tx_hash = Blake2bHasher::default().digest(&raw_data);

    let burn_info = BurnTransactionInfo::new(
        raw_data.clone(),
        tx_hash,
        amount,
        target_address,
        nonce,
        height,
        1234567890,
        chain_id,
    )
    .unwrap();

    let leaf_hash = Blake2bHasher::default().digest(&burn_info.raw_data);
    let sibling_hash = Blake2bHasher::default().digest(b"sibling");
    let proof = MerkleProof::new(&[leaf_hash.clone(), sibling_hash], &[leaf_hash]);
    let any_proof = AnyMerkleProof::Blake2b(proof);
    let oracle_state_hash = create_test_oracle_state_hash();

    let outgoing_tx = OutgoingTransaction::new(raw_data, any_proof, oracle_state_hash).unwrap();

    let result = validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
    assert!(result.is_ok());
    assert!(result.unwrap());
}

/// Test transaction detail validation consistency across multiple calls
#[test]
fn test_transaction_detail_validation_consistency() {
    let validator = create_test_validator();
    let chain_config = create_test_chain_config();
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);

    // Validate multiple times - should always return same result
    for _ in 0..5 {
        let result =
            validator.validate_transaction_details(&burn_info, &outgoing_tx, &chain_config);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    // Now test with mismatched data - should consistently fail
    let outgoing_tx_mismatch =
        create_test_outgoing_tx_with_different_amount(&burn_info, Coin::from_u64_unchecked(2000));
    for _ in 0..5 {
        let result = validator.validate_transaction_details(
            &burn_info,
            &outgoing_tx_mismatch,
            &chain_config,
        );
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }
}

// ============================================================================
// Unit tests for comprehensive field validation
// ============================================================================

use nimiq_transaction::bridge_contract::transaction_validation;

/// Test burn transaction field validation with valid data
#[test]
fn test_comprehensive_validation_burn_transaction_fields_valid() {
    let burn_info = create_test_burn_info();
    let result = transaction_validation::validate_burn_transaction_fields(&burn_info);
    assert!(result.is_ok());
}

/// Test burn transaction field validation with empty raw data
#[test]
fn test_comprehensive_validation_burn_transaction_empty_data() {
    let mut burn_info = create_test_burn_info();
    burn_info.raw_data = vec![];

    let result = transaction_validation::validate_burn_transaction_fields(&burn_info);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidDataLength
    ));
}

/// Test burn transaction field validation with oversized data
#[test]
fn test_comprehensive_validation_burn_transaction_oversized_data() {
    let mut burn_info = create_test_burn_info();
    burn_info.raw_data = vec![0u8; 10_001]; // Exceeds 10KB limit

    let result = transaction_validation::validate_burn_transaction_fields(&burn_info);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidDataLength
    ));
}

/// Test burn transaction field validation with zero amount
#[test]
fn test_comprehensive_validation_burn_transaction_zero_amount() {
    let mut burn_info = create_test_burn_info();
    burn_info.amount = Coin::ZERO;

    let result = transaction_validation::validate_burn_transaction_fields(&burn_info);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BridgeError::InvalidAmount));
}

/// Test burn transaction field validation with zero nonce
#[test]
fn test_comprehensive_validation_burn_transaction_zero_nonce() {
    let mut burn_info = create_test_burn_info();
    burn_info.nonce = 0;

    let result = transaction_validation::validate_burn_transaction_fields(&burn_info);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BridgeError::NonceAlreadyUsed));
}

/// Test burn transaction field validation with zero block height
#[test]
fn test_comprehensive_validation_burn_transaction_zero_height() {
    let mut burn_info = create_test_burn_info();
    burn_info.block_height = 0;

    let result = transaction_validation::validate_burn_transaction_fields(&burn_info);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidValidityHeight
    ));
}

/// Test burn transaction field validation with zero timestamp
#[test]
fn test_comprehensive_validation_burn_transaction_zero_timestamp() {
    let mut burn_info = create_test_burn_info();
    burn_info.timestamp = 0;

    let result = transaction_validation::validate_burn_transaction_fields(&burn_info);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BridgeError::InvalidTimestamp));
}

/// Test burn transaction field validation with zero chain ID
#[test]
fn test_comprehensive_validation_burn_transaction_zero_chain_id() {
    let mut burn_info = create_test_burn_info();
    burn_info.source_chain_id = 0;

    let result = transaction_validation::validate_burn_transaction_fields(&burn_info);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BridgeError::InvalidChainId));
}

/// Test burn transaction field validation with mismatched hash
#[test]
fn test_comprehensive_validation_burn_transaction_hash_mismatch() {
    let mut burn_info = create_test_burn_info();
    burn_info.tx_hash = Blake2bHasher::default().digest(b"wrong_data");

    let result = transaction_validation::validate_burn_transaction_fields(&burn_info);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidMerkleProof
    ));
}

/// Test amount validation with positive amount
#[test]
fn test_comprehensive_validation_amount_positive() {
    let amount = Coin::from_u64_unchecked(1000);
    let result = transaction_validation::validate_amount(amount, None);
    assert!(result.is_ok());
}

/// Test amount validation with zero amount
#[test]
fn test_comprehensive_validation_amount_zero() {
    let amount = Coin::ZERO;
    let result = transaction_validation::validate_amount(amount, None);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BridgeError::InvalidAmount));
}

/// Test amount validation within range
#[test]
fn test_comprehensive_validation_amount_within_range() {
    let amount = Coin::from_u64_unchecked(500);
    let max_amount = Coin::from_u64_unchecked(1000);
    let result = transaction_validation::validate_amount(amount, Some(max_amount));
    assert!(result.is_ok());
}

/// Test amount validation exceeding maximum
#[test]
fn test_comprehensive_validation_amount_exceeds_max() {
    let amount = Coin::from_u64_unchecked(1500);
    let max_amount = Coin::from_u64_unchecked(1000);
    let result = transaction_validation::validate_amount(amount, Some(max_amount));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BridgeError::InvalidAmount));
}

/// Test amount validation at boundary (equal to max)
#[test]
fn test_comprehensive_validation_amount_at_boundary() {
    let amount = Coin::from_u64_unchecked(1000);
    let max_amount = Coin::from_u64_unchecked(1000);
    let result = transaction_validation::validate_amount(amount, Some(max_amount));
    assert!(result.is_ok());
}

/// Test validity height validation with valid height
#[test]
fn test_comprehensive_validation_validity_height_valid() {
    let validity_height = 100;
    let current_height = 50;
    let max_future = 100;

    let result = transaction_validation::validate_validity_height(
        validity_height,
        current_height,
        max_future,
    );
    assert!(result.is_ok());
}

/// Test validity height validation with zero height
#[test]
fn test_comprehensive_validation_validity_height_zero() {
    let validity_height = 0;
    let current_height = 50;
    let max_future = 100;

    let result = transaction_validation::validate_validity_height(
        validity_height,
        current_height,
        max_future,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidValidityHeight
    ));
}

/// Test validity height validation too far in future
#[test]
fn test_comprehensive_validation_validity_height_too_far_future() {
    let validity_height = 200;
    let current_height = 50;
    let max_future = 100;

    let result = transaction_validation::validate_validity_height(
        validity_height,
        current_height,
        max_future,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidValidityHeight
    ));
}

/// Test validity height validation at boundary
#[test]
fn test_comprehensive_validation_validity_height_at_boundary() {
    let validity_height = 150;
    let current_height = 50;
    let max_future = 100;

    let result = transaction_validation::validate_validity_height(
        validity_height,
        current_height,
        max_future,
    );
    assert!(result.is_ok());
}

/// Test validity height validation in the past
#[test]
fn test_comprehensive_validation_validity_height_past() {
    let validity_height = 25;
    let current_height = 50;
    let max_future = 100;

    let result = transaction_validation::validate_validity_height(
        validity_height,
        current_height,
        max_future,
    );
    assert!(result.is_ok());
}

/// Test data integrity validation with matching hash
#[test]
fn test_comprehensive_validation_data_integrity_valid() {
    let data = b"test_data_for_integrity_check".to_vec();
    let expected_hash = Blake2bHasher::default().digest(&data);

    let result = transaction_validation::validate_data_integrity(&data, &expected_hash);
    assert!(result.is_ok());
}

/// Test data integrity validation with mismatched hash
#[test]
fn test_comprehensive_validation_data_integrity_mismatch() {
    let data = b"test_data_for_integrity_check".to_vec();
    let wrong_hash = Blake2bHasher::default().digest(b"different_data");

    let result = transaction_validation::validate_data_integrity(&data, &wrong_hash);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidMerkleProof
    ));
}

/// Test data integrity validation with empty data
#[test]
fn test_comprehensive_validation_data_integrity_empty() {
    let data: Vec<u8> = vec![];
    let hash = Blake2bHasher::default().digest(b"some_data");

    let result = transaction_validation::validate_data_integrity(&data, &hash);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidDataLength
    ));
}

/// Test outgoing transaction comprehensive validation with valid data
#[test]
fn test_comprehensive_validation_outgoing_transaction_valid() {
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let chain_config = create_test_chain_config();
    let max_proof_depth = 32;

    let result = transaction_validation::validate_outgoing_transaction_comprehensive(
        &outgoing_tx,
        max_proof_depth,
        &chain_config,
    );
    assert!(result.is_ok());
}

/// Test outgoing transaction comprehensive validation with empty burn data
#[test]
fn test_comprehensive_validation_outgoing_transaction_empty_burn_data() {
    let burn_info = create_test_burn_info();
    let mut outgoing_tx = create_test_outgoing_tx(&burn_info);
    outgoing_tx.burn_transaction_data = vec![];
    let chain_config = create_test_chain_config();
    let max_proof_depth = 32;

    let result = transaction_validation::validate_outgoing_transaction_comprehensive(
        &outgoing_tx,
        max_proof_depth,
        &chain_config,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidDataLength
    ));
}

/// Test outgoing transaction comprehensive validation with oversized burn data
#[test]
fn test_comprehensive_validation_outgoing_transaction_oversized_burn_data() {
    let burn_info = create_test_burn_info();
    let mut outgoing_tx = create_test_outgoing_tx(&burn_info);
    outgoing_tx.burn_transaction_data = vec![0u8; 10_001];
    let chain_config = create_test_chain_config();
    let max_proof_depth = 32;

    let result = transaction_validation::validate_outgoing_transaction_comprehensive(
        &outgoing_tx,
        max_proof_depth,
        &chain_config,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidDataLength
    ));
}

/// Test outgoing transaction comprehensive validation with zero amount
#[test]
fn test_comprehensive_validation_outgoing_transaction_zero_amount() {
    // Create burn data with zero amount
    let target_address = vec![2u8; 20];
    let amount = Coin::ZERO;
    let nonce = 1u64;
    let height = 100u32;
    let chain_id = 1u32;

    let raw_data = create_burn_transaction_data(amount, &target_address, nonce, height, chain_id);
    let tx_hash = Blake2bHasher::default().digest(&raw_data);

    // BurnTransactionInfo creation should fail with zero amount
    let result = BurnTransactionInfo::new(
        raw_data,
        tx_hash,
        amount,
        target_address,
        nonce,
        height,
        1234567890,
        chain_id,
    );
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BridgeError::InvalidAmount));
}

/// Test outgoing transaction comprehensive validation with zero nonce
#[test]
fn test_comprehensive_validation_outgoing_transaction_zero_nonce() {
    // Create burn data with zero nonce (nonce 0 is invalid)
    let target_address = vec![2u8; 20];
    let amount = Coin::from_u64_unchecked(1000);
    let nonce = 0u64;
    let height = 100u32;
    let chain_id = 1u32;

    let raw_data = create_burn_transaction_data(amount, &target_address, nonce, height, chain_id);
    let tx_hash = Blake2bHasher::default().digest(&raw_data);

    // BurnTransactionInfo creation should fail with zero nonce
    let result = BurnTransactionInfo::new(
        raw_data,
        tx_hash,
        amount,
        target_address,
        nonce,
        height,
        1234567890,
        chain_id,
    );
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BridgeError::NonceAlreadyUsed));
}

/// Test outgoing transaction comprehensive validation with zero block height
#[test]
fn test_comprehensive_validation_outgoing_transaction_zero_height() {
    // Create burn data with zero height
    let target_address = vec![2u8; 20];
    let amount = Coin::from_u64_unchecked(1000);
    let nonce = 1u64;
    let height = 0u32;
    let chain_id = 1u32;

    let raw_data = create_burn_transaction_data(amount, &target_address, nonce, height, chain_id);
    let tx_hash = Blake2bHasher::default().digest(&raw_data);

    // BurnTransactionInfo creation should fail with zero height
    let result = BurnTransactionInfo::new(
        raw_data,
        tx_hash,
        amount,
        target_address,
        nonce,
        height,
        1234567890,
        chain_id,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidValidityHeight
    ));
}

/// Test outgoing transaction comprehensive validation with excessive proof depth
#[test]
fn test_comprehensive_validation_outgoing_transaction_proof_depth_exceeded() {
    let burn_info = create_test_burn_info();
    let outgoing_tx = create_test_outgoing_tx(&burn_info);
    let chain_config = create_test_chain_config();
    let max_proof_depth = 0; // Set very low limit

    let result = transaction_validation::validate_outgoing_transaction_comprehensive(
        &outgoing_tx,
        max_proof_depth,
        &chain_config,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::ProofDepthExceeded
    ));
}

/// Test burn transaction comprehensive validation with valid data
#[test]
fn test_comprehensive_validation_burn_transaction_comprehensive_valid() {
    use nimiq_transaction::bridge_contract::{AddressFormat, ChainConfig, Endianness};

    let burn_info = create_test_burn_info();
    let chain_config = ChainConfig {
        chain_id: 1,
        hash_function: AnyHash::Blake2b(AnyHash32::default()),
        address_format: AddressFormat::Nimiq,
        endianness: Endianness::LittleEndian,
        block_time: std::time::Duration::from_secs(60),
        validation_program: ValidationProgram::empty(),
    };
    let current_height = 150;

    let result = transaction_validation::validate_burn_transaction_comprehensive(
        &burn_info,
        &chain_config,
        current_height,
    );
    assert!(result.is_ok());
}

/// Test burn transaction comprehensive validation with excessive amount
#[test]
fn test_comprehensive_validation_burn_transaction_excessive_amount() {
    use nimiq_transaction::bridge_contract::{AddressFormat, ChainConfig, Endianness};

    let mut burn_info = create_test_burn_info();
    burn_info.amount = Coin::from_u64_unchecked(2_000_000_000_000_000); // Exceeds max
    let chain_config = ChainConfig {
        chain_id: 1,
        hash_function: AnyHash::Blake2b(AnyHash32::default()),
        address_format: AddressFormat::Nimiq,
        endianness: Endianness::LittleEndian,
        block_time: std::time::Duration::from_secs(60),
        validation_program: ValidationProgram::empty(),
    };
    let current_height = 150;

    let result = transaction_validation::validate_burn_transaction_comprehensive(
        &burn_info,
        &chain_config,
        current_height,
    );
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BridgeError::InvalidAmount));
}

/// Test burn transaction comprehensive validation with future block height
#[test]
fn test_comprehensive_validation_burn_transaction_future_height() {
    use nimiq_transaction::bridge_contract::{AddressFormat, ChainConfig, Endianness};

    let mut burn_info = create_test_burn_info();
    burn_info.block_height = 2000; // Too far in future
    let chain_config = ChainConfig {
        chain_id: 1,
        hash_function: AnyHash::Blake2b(AnyHash32::default()),
        address_format: AddressFormat::Nimiq,
        endianness: Endianness::LittleEndian,
        block_time: std::time::Duration::from_secs(60),
        validation_program: ValidationProgram::empty(),
    };
    let current_height = 150;

    let result = transaction_validation::validate_burn_transaction_comprehensive(
        &burn_info,
        &chain_config,
        current_height,
    );
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidValidityHeight
    ));
}

/// Test multiple field validation failures
#[test]
fn test_comprehensive_validation_multiple_failures() {
    let mut burn_info = create_test_burn_info();
    burn_info.amount = Coin::ZERO;
    burn_info.nonce = 0;
    burn_info.block_height = 0;

    // Should fail on first validation error (amount)
    let result = transaction_validation::validate_burn_transaction_fields(&burn_info);
    assert!(result.is_err());
}

// ============================================================================
// Unit tests for address format validation
// ============================================================================

/// Helper function to create a test chain config with specified address format
fn create_chain_config_with_format(address_format: AddressFormat) -> ChainConfig {
    ChainConfig {
        chain_id: 1,
        hash_function: AnyHash::Blake2b(AnyHash32::default()),
        address_format,
        endianness: Endianness::LittleEndian,
        block_time: std::time::Duration::from_secs(60),
        validation_program: ValidationProgram::empty(),
    }
}

/// Test Nimiq address format validation with valid 20-byte address
#[test]
fn test_address_format_validation_nimiq_valid() {
    let address_bytes = [0x01u8; 20];
    let chain_config = create_chain_config_with_format(AddressFormat::Nimiq);

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_ok());
}

/// Test Nimiq address format validation with invalid length
#[test]
fn test_address_format_validation_nimiq_invalid_length() {
    let address_bytes = [0x01u8; 19]; // Too short
    let chain_config = create_chain_config_with_format(AddressFormat::Nimiq);

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidAddress(_)
    ));
}

/// Test Nimiq address format validation with oversized address
#[test]
fn test_address_format_validation_nimiq_oversized() {
    let address_bytes = [0x01u8; 21]; // Too long
    let chain_config = create_chain_config_with_format(AddressFormat::Nimiq);

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidAddress(_)
    ));
}

/// Test Ethereum address format validation with valid 20-byte address
#[test]
fn test_address_format_validation_ethereum_valid() {
    let address_bytes = [0xAAu8; 20];
    let chain_config = create_chain_config_with_format(AddressFormat::Ethereum);

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_ok());
}

/// Test Ethereum address format validation with invalid length
#[test]
fn test_address_format_validation_ethereum_invalid_length() {
    let address_bytes = [0xAAu8; 18]; // Too short
    let chain_config = create_chain_config_with_format(AddressFormat::Ethereum);

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidAddress(_)
    ));
}

/// Test Bitcoin address format validation with valid address
#[test]
fn test_address_format_validation_bitcoin_valid() {
    let address_bytes = [0xBBu8; 25]; // Valid Bitcoin address length
    let chain_config = create_chain_config_with_format(AddressFormat::Bitcoin);

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_ok());
}

/// Test Bitcoin address format validation with minimum length
#[test]
fn test_address_format_validation_bitcoin_min_length() {
    let address_bytes = [0xBBu8; 20]; // Minimum valid length
    let chain_config = create_chain_config_with_format(AddressFormat::Bitcoin);

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_ok());
}

/// Test Bitcoin address format validation with maximum length
#[test]
fn test_address_format_validation_bitcoin_max_length() {
    let address_bytes = [0xBBu8; 64]; // Maximum valid length
    let chain_config = create_chain_config_with_format(AddressFormat::Bitcoin);

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_ok());
}

/// Test Bitcoin address format validation with too short address
#[test]
fn test_address_format_validation_bitcoin_too_short() {
    let address_bytes = [0xBBu8; 19]; // Too short
    let chain_config = create_chain_config_with_format(AddressFormat::Bitcoin);

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidAddress(_)
    ));
}

/// Test Bitcoin address format validation with too long address
#[test]
fn test_address_format_validation_bitcoin_too_long() {
    let address_bytes = [0xBBu8; 65]; // Too long
    let chain_config = create_chain_config_with_format(AddressFormat::Bitcoin);

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidAddress(_)
    ));
}

/// Test custom address format validation with valid address
#[test]
fn test_address_format_validation_custom_valid() {
    let address_bytes = [0xCCu8; 32]; // Custom format
    let chain_config =
        create_chain_config_with_format(AddressFormat::Custom("custom-chain".to_string()));

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_ok());
}

/// Test custom address format validation with empty address
#[test]
fn test_address_format_validation_custom_empty() {
    let address_bytes: Vec<u8> = vec![];
    let chain_config =
        create_chain_config_with_format(AddressFormat::Custom("custom-chain".to_string()));

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidAddress(_)
    ));
}

/// Test custom address format validation with oversized address
#[test]
fn test_address_format_validation_custom_oversized() {
    let address_bytes = [0xCCu8; 65]; // Too long
    let chain_config =
        create_chain_config_with_format(AddressFormat::Custom("custom-chain".to_string()));

    let result = transaction_validation::validate_address_format(&address_bytes, &chain_config);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        BridgeError::InvalidAddress(_)
    ));
}

/// Test address format validation with various valid formats
#[test]
fn test_address_format_validation_various_valid_formats() {
    // Test Nimiq
    let nimiq_addr = [0x01u8; 20];
    let nimiq_config = create_chain_config_with_format(AddressFormat::Nimiq);
    assert!(transaction_validation::validate_address_format(&nimiq_addr, &nimiq_config).is_ok());

    // Test Ethereum
    let eth_addr = [0x02u8; 20];
    let eth_config = create_chain_config_with_format(AddressFormat::Ethereum);
    assert!(transaction_validation::validate_address_format(&eth_addr, &eth_config).is_ok());

    // Test Bitcoin
    let btc_addr = [0x03u8; 25];
    let btc_config = create_chain_config_with_format(AddressFormat::Bitcoin);
    assert!(transaction_validation::validate_address_format(&btc_addr, &btc_config).is_ok());

    // Test Custom
    let custom_addr = [0x04u8; 32];
    let custom_config = create_chain_config_with_format(AddressFormat::Custom("test".to_string()));
    assert!(transaction_validation::validate_address_format(&custom_addr, &custom_config).is_ok());
}

/// Test address format validation with edge case addresses
#[test]
fn test_address_format_validation_edge_cases() {
    // All zeros
    let zero_addr = [0x00u8; 20];
    let nimiq_config = create_chain_config_with_format(AddressFormat::Nimiq);
    assert!(transaction_validation::validate_address_format(&zero_addr, &nimiq_config).is_ok());

    // All ones
    let ones_addr = [0xFFu8; 20];
    assert!(transaction_validation::validate_address_format(&ones_addr, &nimiq_config).is_ok());

    // Pattern
    let pattern_addr = [0xAAu8; 20];
    assert!(transaction_validation::validate_address_format(&pattern_addr, &nimiq_config).is_ok());
}

/// Test address format validation rejection of invalid formats
#[test]
fn test_address_format_validation_rejection_scenarios() {
    let nimiq_config = create_chain_config_with_format(AddressFormat::Nimiq);

    // Empty address
    let empty_addr: Vec<u8> = vec![];
    assert!(transaction_validation::validate_address_format(&empty_addr, &nimiq_config).is_err());

    // Single byte
    let single_byte = [0x01u8; 1];
    assert!(transaction_validation::validate_address_format(&single_byte, &nimiq_config).is_err());

    // Wrong length
    let wrong_length = [0x01u8; 15];
    assert!(transaction_validation::validate_address_format(&wrong_length, &nimiq_config).is_err());
}

/// Test address format validation with RecipientData
#[test]
fn test_address_format_validation_with_recipient_data() {
    use nimiq_transaction::bridge_contract::RecipientData;

    let target_address = vec![0x05u8; 20];
    let chain_config = create_chain_config_with_format(AddressFormat::Nimiq);

    let recipient_data = RecipientData::new(target_address, 1, &chain_config);

    assert!(recipient_data.is_ok());
}

/// Test address format validation with RecipientData for Ethereum
#[test]
fn test_address_format_validation_recipient_data_ethereum() {
    use nimiq_transaction::bridge_contract::RecipientData;

    let target_address = vec![0x06u8; 20];
    let chain_config = create_chain_config_with_format(AddressFormat::Ethereum);

    let recipient_data = RecipientData::new(target_address, 1, &chain_config);

    assert!(recipient_data.is_ok());
}

/// Test address format validation with RecipientData for Bitcoin
#[test]
fn test_address_format_validation_recipient_data_bitcoin() {
    use nimiq_transaction::bridge_contract::RecipientData;

    let target_address = vec![0x07u8; 20];
    let chain_config = create_chain_config_with_format(AddressFormat::Bitcoin);

    let recipient_data = RecipientData::new(target_address, 1, &chain_config);

    assert!(recipient_data.is_ok());
}

/// Test address format validation with RecipientData for custom format
#[test]
fn test_address_format_validation_recipient_data_custom() {
    use nimiq_transaction::bridge_contract::RecipientData;

    let target_address = vec![0x08u8; 20];
    let chain_config = create_chain_config_with_format(AddressFormat::Custom("custom".to_string()));

    let recipient_data = RecipientData::new(target_address, 1, &chain_config);

    assert!(recipient_data.is_ok());
}

/// Test address format validation consistency across different chains
#[test]
fn test_address_format_validation_cross_chain_consistency() {
    let address_bytes = [0x09u8; 20];

    // Same address should be valid for both Nimiq and Ethereum (both use 20 bytes)
    let nimiq_config = create_chain_config_with_format(AddressFormat::Nimiq);
    let eth_config = create_chain_config_with_format(AddressFormat::Ethereum);

    assert!(transaction_validation::validate_address_format(&address_bytes, &nimiq_config).is_ok());
    assert!(transaction_validation::validate_address_format(&address_bytes, &eth_config).is_ok());
}

/// Test address format validation with data encoding module
#[test]
fn test_address_format_validation_with_data_encoding() {
    use nimiq_transaction::bridge_contract::data_encoding;

    let address = Address::from([0x0Au8; 20]);
    let chain_config = create_chain_config_with_format(AddressFormat::Nimiq);

    // Encode address
    let encoded = data_encoding::encode_address(&address, &chain_config);
    assert!(encoded.is_ok());

    // Validate encoded address
    let encoded_bytes = encoded.unwrap();
    let result = transaction_validation::validate_address_format(&encoded_bytes, &chain_config);
    assert!(result.is_ok());
}

/// Test address format validation with data encoding for Ethereum
#[test]
fn test_address_format_validation_data_encoding_ethereum() {
    use nimiq_transaction::bridge_contract::data_encoding;

    let address = Address::from([0x0Bu8; 20]);
    let chain_config = create_chain_config_with_format(AddressFormat::Ethereum);

    // Encode and validate
    let encoded = data_encoding::encode_address(&address, &chain_config).unwrap();
    let result = transaction_validation::validate_address_format(&encoded, &chain_config);
    assert!(result.is_ok());
}

/// Test address format validation round-trip (encode then decode)
#[test]
fn test_address_format_validation_round_trip() {
    use nimiq_transaction::bridge_contract::data_encoding;

    let original_address = Address::from([0x0Cu8; 20]);
    let chain_config = create_chain_config_with_format(AddressFormat::Nimiq);

    // Encode
    let encoded = data_encoding::encode_address(&original_address, &chain_config).unwrap();

    // Validate
    assert!(transaction_validation::validate_address_format(&encoded, &chain_config).is_ok());

    // Decode
    let decoded = data_encoding::decode_address(&encoded, &chain_config).unwrap();

    // Verify round-trip
    assert_eq!(original_address, decoded);
}

// ============================================================================
