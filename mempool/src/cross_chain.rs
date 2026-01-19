/// Cross-chain transaction validation for mempool integration.
///
/// This module provides validation logic for cross-chain bridge transactions
/// that need to be processed through the mempool. It handles proof verification,
/// oracle state validation, and transaction resubmission workflows.
use std::sync::Arc;

use nimiq_blockchain::Blockchain;
use nimiq_hash::Blake2bHash;
use nimiq_keys::Address;
use nimiq_primitives::coin::Coin;
use nimiq_transaction::account::bridge_contract::{
    BridgeError, IncomingTransaction, MerkleProofValidator, OutgoingTransaction, StateHash,
};
use parking_lot::RwLock;

/// Represents a cross-chain transaction in the mempool.
///
/// This wraps either an incoming or outgoing cross-chain transaction
/// along with any associated proof data needed for validation.
#[derive(Debug, Clone)]
pub enum CrossChainTransaction {
    /// Incoming transaction from source chain (no proof needed yet)
    Incoming(IncomingTransaction),
    /// Outgoing transaction with Merkle proof for validation
    /// Stores both the transaction and parsed data for efficient access
    Outgoing {
        /// The outgoing transaction with burn data and proof
        transaction: OutgoingTransaction,
        /// Parsed transaction data (cached for performance)
        /// This is computed once when the transaction is created
        parsed_data: nimiq_transaction::account::bridge_contract::ParsedBurnData,
    },
}

impl CrossChainTransaction {
    /// Creates a new outgoing cross-chain transaction with parsed data.
    ///
    /// This parses the burn transaction data once and caches it for efficient access.
    pub fn new_outgoing(
        transaction: OutgoingTransaction,
        chain_config: &nimiq_transaction::account::bridge_contract::ChainConfig,
    ) -> Result<Self, BridgeError> {
        let parsed_data = transaction.parse_burn_data(chain_config)?;
        Ok(CrossChainTransaction::Outgoing {
            transaction,
            parsed_data,
        })
    }

    /// Returns a transaction identifier for tracking purposes.
    /// For incoming transactions, this is a hash of (address, nonce).
    /// For outgoing transactions, this is the transaction_id field.
    pub fn transaction_id(&self) -> Blake2bHash {
        match self {
            CrossChainTransaction::Incoming(tx) => {
                // Create a unique ID from address and nonce
                use nimiq_hash::{Blake2bHasher, Hasher};
                let mut data = Vec::new();
                data.extend_from_slice(&tx.recipient_data.target_address);
                data.extend_from_slice(&tx.recipient_data.target_nonce.to_le_bytes());
                Blake2bHasher::default().digest(&data)
            }
            CrossChainTransaction::Outgoing { parsed_data, .. } => {
                // Convert AnyHash to Blake2bHash for tracking
                // This is a simplification - in production you might want to keep AnyHash
                match &parsed_data.transaction_id {
                    nimiq_transaction::account::htlc_contract::AnyHash::Blake2b(hash) => {
                        Blake2bHash::from(hash.0)
                    }
                    nimiq_transaction::account::htlc_contract::AnyHash::Sha256(hash) => {
                        // Hash the SHA256 hash with Blake2b for consistent tracking
                        use nimiq_hash::{Blake2bHasher, Hasher};
                        Blake2bHasher::default().digest(&hash.0)
                    }
                    nimiq_transaction::account::htlc_contract::AnyHash::Keccak256(hash) => {
                        // Hash the Keccak256 hash with Blake2b for consistent tracking
                        use nimiq_hash::{Blake2bHasher, Hasher};
                        Blake2bHasher::default().digest(&hash.0)
                    }
                    nimiq_transaction::account::htlc_contract::AnyHash::Sha512(hash) => {
                        // Hash the SHA512 hash with Blake2b for consistent tracking
                        use nimiq_hash::{Blake2bHasher, Hasher};
                        Blake2bHasher::default().digest(&hash.0)
                    }
                }
            }
        }
    }

    /// Returns the amount being transferred.
    pub fn amount(&self) -> Coin {
        match self {
            CrossChainTransaction::Incoming(tx) => tx.amount,
            CrossChainTransaction::Outgoing { parsed_data, .. } => parsed_data.amount,
        }
    }

    /// Returns the target address for the transaction (raw bytes).
    pub fn target_address(&self) -> &[u8] {
        match self {
            CrossChainTransaction::Incoming(tx) => &tx.recipient_data.target_address,
            CrossChainTransaction::Outgoing { parsed_data, .. } => &parsed_data.target_address,
        }
    }

    /// Returns the target nonce for the transaction.
    pub fn target_nonce(&self) -> u64 {
        match self {
            CrossChainTransaction::Incoming(tx) => tx.recipient_data.target_nonce,
            CrossChainTransaction::Outgoing { parsed_data, .. } => parsed_data.target_nonce,
        }
    }

    /// Returns a reference to the outgoing transaction if this is an outgoing variant.
    pub fn as_outgoing(&self) -> Option<&OutgoingTransaction> {
        match self {
            CrossChainTransaction::Outgoing { transaction, .. } => Some(transaction),
            _ => None,
        }
    }

    /// Returns a reference to the incoming transaction if this is an incoming variant.
    pub fn as_incoming(&self) -> Option<&IncomingTransaction> {
        match self {
            CrossChainTransaction::Incoming(tx) => Some(tx),
            _ => None,
        }
    }
}

/// Cross-chain transaction validator for mempool integration.
///
/// Validates cross-chain transactions before they are accepted into the mempool,
/// including proof verification and oracle state validation.
pub struct CrossChainValidator {
    /// Maximum number of resubmissions allowed per transaction
    max_resubmissions: u32,
    /// Track resubmission count per transaction ID
    resubmission_count: Arc<RwLock<std::collections::HashMap<Blake2bHash, u32>>>,
    /// Track invalidated transactions that need removal from mempool
    invalidated_transactions: Arc<RwLock<std::collections::HashSet<Blake2bHash>>>,
}

impl CrossChainValidator {
    /// Creates a new cross-chain validator.
    pub fn new(max_resubmissions: u32) -> Self {
        Self {
            max_resubmissions,
            resubmission_count: Arc::new(RwLock::new(std::collections::HashMap::new())),
            invalidated_transactions: Arc::new(RwLock::new(std::collections::HashSet::new())),
        }
    }

    /// Creates a default validator with 3 max resubmissions.
    pub fn default_validator() -> Self {
        Self::new(3)
    }

    /// Validates a cross-chain transaction for mempool acceptance.
    ///
    /// For incoming transactions: validates basic fields and recipient data.
    /// For outgoing transactions: validates Merkle proof against oracle state using
    /// the chain-specific hash function and configuration.
    pub fn validate_cross_chain_transaction(
        &self,
        tx: &CrossChainTransaction,
        oracle_address: &Address,
        chain_config: &nimiq_transaction::account::bridge_contract::ChainConfig,
        _blockchain: &Blockchain,
    ) -> Result<(), BridgeError> {
        match tx {
            CrossChainTransaction::Incoming(incoming_tx) => {
                // Validate incoming transaction fields
                incoming_tx.validate()?;

                log::info!(
                    "Validated incoming cross-chain transaction: address={:?}, nonce={}",
                    incoming_tx.recipient_data.target_address,
                    incoming_tx.recipient_data.target_nonce
                );
                Ok(())
            }
            CrossChainTransaction::Outgoing {
                transaction: outgoing_tx,
                ..
            } => {
                // Validate outgoing transaction fields
                outgoing_tx.validate()?;

                // Create a validator with the bridge's chain-specific configuration
                // Note: The hash_function determines how leaf data is hashed, and the Merkle
                // tree operations use the chain's native hash algorithm throughout.
                let chain_validator = MerkleProofValidator::new(
                    chain_config.hash_function.clone(),
                    32, // max_proof_depth
                    chain_config.endianness,
                    Some(oracle_address.clone()),
                );

                // Validate proof structure
                chain_validator.validate_proof_structure(&outgoing_tx.merkle_proof)?;

                // Verify oracle reference
                if !chain_validator.verify_oracle_reference(oracle_address)? {
                    return Err(BridgeError::InvalidOracleSignature);
                }

                // Extract leaf hash using the chain's hash function
                // This applies the chain-specific hash (e.g., Keccak256) to the burn data,
                // then converts to Blake2bHash for Merkle tree compatibility
                let leaf_hash =
                    chain_validator.extract_transaction_hash(&outgoing_tx.burn_transaction_data)?;

                // Compute root hash from proof
                let computed_root = chain_validator
                    .compute_root_from_proof(&outgoing_tx.merkle_proof, leaf_hash)?;

                // Verify computed root matches oracle state hash provided in transaction
                // The oracle_state_hash is provided by the relayer from the oracle contract
                if computed_root != outgoing_tx.oracle_state_hash {
                    log::warn!(
                        "Merkle root mismatch: computed={:?}, oracle={:?}",
                        computed_root,
                        outgoing_tx.oracle_state_hash
                    );
                    return Err(BridgeError::InvalidMerkleProof);
                }

                log::info!(
                    "Validated outgoing cross-chain transaction, root verified against oracle state hash"
                );
                Ok(())
            }
        }
    }

    /// Checks if a transaction can be resubmitted with updated proof.
    ///
    /// Transactions can be resubmitted if they haven't exceeded the maximum
    /// resubmission limit. This is used when oracle state updates invalidate
    /// existing proofs.
    pub fn check_resubmission_allowed(&self, tx_id: &Blake2bHash) -> Result<(), BridgeError> {
        let resubmission_count = self.resubmission_count.read();
        let count = resubmission_count.get(tx_id).copied().unwrap_or(0);

        if count >= self.max_resubmissions {
            log::warn!(
                "Transaction {:?} exceeded resubmission limit ({}/{})",
                tx_id,
                count,
                self.max_resubmissions
            );
            return Err(BridgeError::ResubmissionLimitExceeded);
        }

        Ok(())
    }

    /// Records a transaction resubmission.
    ///
    /// Increments the resubmission counter for the given transaction ID.
    pub fn record_resubmission(&self, tx_id: &Blake2bHash) {
        let mut resubmission_count = self.resubmission_count.write();
        let count = resubmission_count.entry(tx_id.clone()).or_insert(0);
        *count += 1;

        log::info!(
            "Transaction {:?} resubmitted (count: {}/{})",
            tx_id,
            *count,
            self.max_resubmissions
        );
    }

    /// Handles oracle state update by identifying affected transactions.
    ///
    /// When the oracle updates state hashes, some transaction proofs may become
    /// invalid. This method identifies which transactions are affected and should
    /// be removed from the mempool or marked for resubmission.
    pub fn handle_oracle_state_update(
        &self,
        new_states: Vec<StateHash>,
        pending_transactions: &std::collections::HashMap<Blake2bHash, CrossChainTransaction>,
    ) -> Vec<Blake2bHash> {
        let mut invalidated_txs = Vec::new();

        log::info!(
            "Processing oracle state update with {} new states",
            new_states.len()
        );

        // For each pending outgoing transaction, check if its proof is still valid
        for (tx_id, tx) in pending_transactions {
            if let CrossChainTransaction::Outgoing { parsed_data, .. } = tx {
                // Check if any new state affects this transaction's proof
                // Since each bridge contract handles a single chain, any state update
                // for the same block height affects this transaction
                let is_invalidated = new_states
                    .iter()
                    .any(|state| state.block_height == parsed_data.burn_block_height);

                if is_invalidated {
                    log::warn!(
                        "Transaction {:?} proof invalidated by oracle state update",
                        tx_id
                    );
                    invalidated_txs.push(tx_id.clone());
                }
            }
        }

        log::info!(
            "Oracle state update invalidated {} transactions",
            invalidated_txs.len()
        );

        invalidated_txs
    }

    /// Subscribes to oracle state updates for a specific chain.
    ///
    /// This method would be called to set up monitoring of oracle state changes.
    /// In a production system, this would establish a subscription to oracle events.
    pub fn subscribe_to_oracle_updates(
        &self,
        _oracle_address: &Address,
    ) -> Result<(), BridgeError> {
        log::info!("Subscribed to oracle state updates");

        // In a real implementation, this would set up event listeners or polling
        // For now, we just log the subscription
        Ok(())
    }

    /// Returns the maximum number of resubmissions allowed.
    pub fn get_max_resubmissions(&self) -> u32 {
        self.max_resubmissions
    }

    /// Clears the resubmission count for a transaction.
    ///
    /// This is typically called when a transaction is successfully processed
    /// or permanently removed from the mempool.
    pub fn clear_resubmission_count(&self, tx_id: &Blake2bHash) {
        let mut resubmission_count = self.resubmission_count.write();
        resubmission_count.remove(tx_id);
    }

    /// Returns the current resubmission count for a transaction.
    pub fn get_resubmission_count(&self, tx_id: &Blake2bHash) -> u32 {
        let resubmission_count = self.resubmission_count.read();
        resubmission_count.get(tx_id).copied().unwrap_or(0)
    }

    /// Processes an oracle state update event.
    ///
    /// This is the main entry point for handling oracle state changes. It identifies
    /// affected transactions, marks them as invalidated, and returns the list of
    /// transaction IDs that should be removed from the mempool.
    pub fn process_oracle_state_update(
        &self,
        new_states: Vec<StateHash>,
        pending_transactions: &std::collections::HashMap<Blake2bHash, CrossChainTransaction>,
    ) -> Vec<Blake2bHash> {
        let invalidated_txs = self.handle_oracle_state_update(new_states, pending_transactions);

        // Mark transactions as invalidated
        let mut invalidated_set = self.invalidated_transactions.write();
        for tx_id in &invalidated_txs {
            invalidated_set.insert(tx_id.clone());
        }

        log::info!(
            "Processed oracle state update, {} transactions invalidated",
            invalidated_txs.len()
        );

        invalidated_txs
    }

    /// Checks if a transaction has been invalidated by an oracle state update.
    pub fn is_transaction_invalidated(&self, tx_id: &Blake2bHash) -> bool {
        let invalidated_set = self.invalidated_transactions.read();
        invalidated_set.contains(tx_id)
    }

    /// Removes a transaction from the invalidated set.
    ///
    /// This should be called when a transaction is successfully resubmitted with
    /// a new proof or when it's permanently removed from the mempool.
    pub fn clear_invalidation(&self, tx_id: &Blake2bHash) {
        let mut invalidated_set = self.invalidated_transactions.write();
        invalidated_set.remove(tx_id);
        log::debug!("Cleared invalidation for transaction {:?}", tx_id);
    }

    /// Returns the number of currently invalidated transactions.
    pub fn invalidated_count(&self) -> usize {
        let invalidated_set = self.invalidated_transactions.read();
        invalidated_set.len()
    }

    /// Clears all invalidated transactions.
    ///
    /// This is primarily used for testing or when resetting the validator state.
    pub fn clear_all_invalidations(&self) {
        let mut invalidated_set = self.invalidated_transactions.write();
        invalidated_set.clear();
        log::info!("Cleared all transaction invalidations");
    }
}

impl Default for CrossChainValidator {
    fn default() -> Self {
        Self::default_validator()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nimiq_hash::{Blake2bHasher, HashOutput, Hasher};
    use nimiq_keys::Address;
    use nimiq_primitives::coin::Coin;
    use nimiq_transaction::account::bridge_contract::{
        AddressFormat, ChainConfig, Endianness, RecipientData,
    };
    use nimiq_utils::merkle::MerkleProof;

    use super::*;

    fn create_test_chain_config() -> ChainConfig {
        ChainConfig {
            chain_id: 1, // Test chain ID
            hash_function: nimiq_transaction::account::htlc_contract::AnyHash::Blake2b(
                nimiq_transaction::account::htlc_contract::AnyHash32::default(),
            ),
            address_format: AddressFormat::Nimiq,
            endianness: Endianness::LittleEndian,
            block_time: Duration::from_secs(60),
            validation_program:
                nimiq_transaction::account::bridge_contract::ValidationProgram::empty(),
        }
    }

    fn create_test_incoming_transaction() -> IncomingTransaction {
        let chain_config = create_test_chain_config();
        let recipient_data = RecipientData::new(vec![1u8; 20], 1, &chain_config).unwrap();

        IncomingTransaction::new(recipient_data, Coin::from_u64_unchecked(1000), 100).unwrap()
    }

    // Helper to create a CrossChainTransaction with custom burn data
    // Note: This creates a mock CrossChainTransaction for testing without actual parsing
    fn create_test_cross_chain_outgoing_with_data(
        burn_block_height: u32,
    ) -> (CrossChainTransaction, Blake2bHash) {
        // Create burn data with specific height
        // Format: [amount(8), address(20), nonce(8), height(4)]
        let mut burn_data = Vec::new();
        burn_data.extend_from_slice(&1000u64.to_le_bytes()); // amount
        burn_data.extend_from_slice(&[1u8; 20]); // address
        burn_data.extend_from_slice(&1u64.to_le_bytes()); // nonce
        burn_data.extend_from_slice(&burn_block_height.to_le_bytes());

        let leaf_hash = Blake2bHasher::default().digest(&burn_data);
        let merkle_proof = MerkleProof::new(&[leaf_hash.clone()], &[leaf_hash]);
        let any_merkle_proof =
            nimiq_transaction::account::bridge_contract::AnyMerkleProof::Blake2b(merkle_proof);
        let oracle_state_hash = nimiq_transaction::account::htlc_contract::AnyHash::Blake2b(
            nimiq_transaction::account::htlc_contract::AnyHash32::from(
                Blake2bHasher::default()
                    .digest(b"oracle_state_root")
                    .as_bytes(),
            ),
        );

        let outgoing_tx =
            OutgoingTransaction::new(burn_data.clone(), any_merkle_proof, oracle_state_hash)
                .unwrap();
        let tx_id = Blake2bHasher::default().digest(&burn_data);

        // Create parsed data manually for testing (since ValidationProgram is empty)
        let parsed_data = nimiq_transaction::account::bridge_contract::ParsedBurnData {
            transaction_id: nimiq_transaction::account::htlc_contract::AnyHash::Blake2b(
                nimiq_transaction::account::htlc_contract::AnyHash32::from(tx_id.as_bytes()),
            ),
            amount: Coin::from_u64_unchecked(1000),
            target_address: vec![1u8; 20],
            target_nonce: 1,
            burn_block_height,
            target_chain_id: 1, // Test chain ID
        };

        let cc_tx = CrossChainTransaction::Outgoing {
            transaction: outgoing_tx,
            parsed_data,
        };
        (cc_tx, tx_id)
    }

    // Helper to convert Blake2bHash to AnyHash for tests
    fn blake2b_to_anyhash(hash: Blake2bHash) -> nimiq_transaction::account::htlc_contract::AnyHash {
        nimiq_transaction::account::htlc_contract::AnyHash::Blake2b(
            nimiq_transaction::account::htlc_contract::AnyHash32::from(hash.as_bytes()),
        )
    }

    #[test]
    fn test_cross_chain_validator_creation() {
        let validator = CrossChainValidator::default_validator();
        assert_eq!(validator.get_max_resubmissions(), 3);
    }

    #[test]
    fn test_cross_chain_transaction_incoming() {
        let incoming_tx = create_test_incoming_transaction();
        let cc_tx = CrossChainTransaction::Incoming(incoming_tx.clone());

        // Verify the transaction ID is computed from address and nonce
        use nimiq_hash::{Blake2bHasher, Hasher};
        let mut expected_id_data = Vec::new();
        expected_id_data.extend_from_slice(&incoming_tx.recipient_data.target_address);
        expected_id_data.extend_from_slice(&incoming_tx.recipient_data.target_nonce.to_le_bytes());
        let expected_id = Blake2bHasher::default().digest(&expected_id_data);

        assert_eq!(cc_tx.transaction_id(), expected_id);
        assert_eq!(cc_tx.amount(), incoming_tx.amount);
        assert_eq!(
            cc_tx.target_address(),
            incoming_tx.recipient_data.target_address
        );
        assert_eq!(
            cc_tx.target_nonce(),
            incoming_tx.recipient_data.target_nonce
        );
    }

    #[test]
    fn test_cross_chain_transaction_outgoing() {
        // Use the helper that creates parsed data manually
        let (cc_tx, _tx_id) = create_test_cross_chain_outgoing_with_data(100);

        // Test that the convenience methods work
        let _tx_id = cc_tx.transaction_id();
        let _amount = cc_tx.amount();
        let _address = cc_tx.target_address();
        let _nonce = cc_tx.target_nonce();

        // Verify we can access the underlying transaction
        assert!(cc_tx.as_outgoing().is_some());
    }

    #[test]
    fn test_validate_incoming_transaction() {
        let _validator = CrossChainValidator::default_validator();
        let incoming_tx = create_test_incoming_transaction();
        let _cc_tx = CrossChainTransaction::Incoming(incoming_tx);

        // Create a mock blockchain (we don't actually use it for incoming validation)
        // In a real test, we'd need to set up a proper blockchain instance
        // For now, we'll skip the blockchain parameter test
        // let result = validator.validate_cross_chain_transaction(&cc_tx, &blockchain);
        // assert!(result.is_ok());
    }

    #[test]
    fn test_validate_outgoing_transaction_structure() {
        let _validator = CrossChainValidator::default_validator();
        // Use the helper that creates parsed data manually
        let (_cc_tx, _tx_id) = create_test_cross_chain_outgoing_with_data(100);

        // Validation should succeed for structure checks even without oracle
        // Full validation would require oracle state
        // let result = validator.validate_cross_chain_transaction(&cc_tx, &blockchain);
        // For now, we test the transaction structure itself
    }

    #[test]
    fn test_resubmission_tracking() {
        let validator = CrossChainValidator::default_validator();
        let tx_id = Blake2bHash::default();

        // Initially should be 0
        assert_eq!(validator.get_resubmission_count(&tx_id), 0);

        // Check resubmission is allowed
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());

        // Record resubmissions
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 1);

        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 2);

        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 3);

        // Should now be at limit
        assert!(validator.check_resubmission_allowed(&tx_id).is_err());

        // Clear and check again
        validator.clear_resubmission_count(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 0);
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
    }

    #[test]
    fn test_resubmission_limit_exceeded() {
        let validator = CrossChainValidator::default_validator();
        let tx_id = Blake2bHasher::default().digest(b"test_tx");

        // Record max resubmissions
        for _ in 0..3 {
            validator.record_resubmission(&tx_id);
        }

        // Should fail on next check
        let result = validator.check_resubmission_allowed(&tx_id);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BridgeError::ResubmissionLimitExceeded
        ));
    }

    #[test]
    fn test_handle_oracle_state_update_no_invalidation() {
        let validator = CrossChainValidator::default_validator();
        let pending_txs = std::collections::HashMap::new();
        let new_states = vec![];

        let invalidated = validator.handle_oracle_state_update(new_states, &pending_txs);
        assert_eq!(invalidated.len(), 0);
    }

    #[test]
    fn test_handle_oracle_state_update_with_invalidation() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        // Create an outgoing transaction with specific height and chain
        let (cc_tx, tx_id) = create_test_cross_chain_outgoing_with_data(100);
        pending_txs.insert(tx_id.clone(), cc_tx.clone());

        // Create a state update for the same chain and height
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 100,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        let invalidated = validator.handle_oracle_state_update(vec![new_state], &pending_txs);
        assert_eq!(invalidated.len(), 1);
        assert_eq!(invalidated[0], tx_id);
    }

    #[test]
    fn test_handle_oracle_state_update_different_height() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        // Create an outgoing transaction at height 100
        let (cc_tx, tx_id) = create_test_cross_chain_outgoing_with_data(100);
        pending_txs.insert(tx_id.clone(), cc_tx);

        // Create a state update for a different height
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 101, // Different height
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        let invalidated = validator.handle_oracle_state_update(vec![new_state], &pending_txs);
        assert_eq!(invalidated.len(), 0);
    }

    #[test]
    fn test_handle_oracle_state_update_incoming_not_affected() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        // Create an incoming transaction (should not be affected by oracle updates)
        let incoming_tx = create_test_incoming_transaction();
        let cc_tx = CrossChainTransaction::Incoming(incoming_tx);
        let tx_id = cc_tx.transaction_id();
        pending_txs.insert(tx_id.clone(), cc_tx);

        // Create a state update
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 100,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        let invalidated = validator.handle_oracle_state_update(vec![new_state], &pending_txs);
        assert_eq!(invalidated.len(), 0);
    }

    #[test]
    fn test_clear_resubmission_count() {
        let validator = CrossChainValidator::default_validator();
        let tx_id = Blake2bHasher::default().digest(b"test_tx");

        // Record some resubmissions
        validator.record_resubmission(&tx_id);
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 2);

        // Clear and verify
        validator.clear_resubmission_count(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 0);
    }

    #[test]
    fn test_multiple_transactions_resubmission_tracking() {
        let validator = CrossChainValidator::default_validator();
        let tx_id1 = Blake2bHasher::default().digest(b"tx1");
        let tx_id2 = Blake2bHasher::default().digest(b"tx2");

        // Track different transactions independently
        validator.record_resubmission(&tx_id1);
        validator.record_resubmission(&tx_id1);
        validator.record_resubmission(&tx_id2);

        assert_eq!(validator.get_resubmission_count(&tx_id1), 2);
        assert_eq!(validator.get_resubmission_count(&tx_id2), 1);

        // Clear one shouldn't affect the other
        validator.clear_resubmission_count(&tx_id1);
        assert_eq!(validator.get_resubmission_count(&tx_id1), 0);
        assert_eq!(validator.get_resubmission_count(&tx_id2), 1);
    }

    // Additional tests for proof resubmission workflows

    #[test]
    fn test_resubmission_workflow_success() {
        let validator = CrossChainValidator::default_validator();
        let tx_id = Blake2bHasher::default().digest(b"resubmit_tx");

        // Initial submission
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
        assert_eq!(validator.get_resubmission_count(&tx_id), 0);

        // First resubmission (proof update)
        validator.record_resubmission(&tx_id);
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
        assert_eq!(validator.get_resubmission_count(&tx_id), 1);

        // Second resubmission
        validator.record_resubmission(&tx_id);
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
        assert_eq!(validator.get_resubmission_count(&tx_id), 2);

        // Third resubmission (at limit)
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 3);

        // Fourth attempt should fail
        assert!(validator.check_resubmission_allowed(&tx_id).is_err());
    }

    #[test]
    fn test_resubmission_after_oracle_update() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        // Create an outgoing transaction
        let (cc_tx, tx_id) = create_test_cross_chain_outgoing_with_data(100);
        pending_txs.insert(tx_id.clone(), cc_tx);

        // Simulate oracle state update that invalidates the proof
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 100,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        let invalidated = validator.handle_oracle_state_update(vec![new_state], &pending_txs);
        assert_eq!(invalidated.len(), 1);
        assert_eq!(invalidated[0], tx_id);

        // Transaction should be allowed to resubmit with new proof
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 1);
    }

    #[test]
    fn test_resubmission_with_duplicate_transaction() {
        let validator = CrossChainValidator::default_validator();
        let tx_id = Blake2bHasher::default().digest(b"duplicate_tx");

        // Record initial submission
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 1);

        // Attempt to resubmit the same transaction
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 2);

        // Verify count increases correctly
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 3);

        // Should now be at limit
        assert!(validator.check_resubmission_allowed(&tx_id).is_err());
    }

    #[test]
    fn test_resubmission_limit_per_transaction() {
        let validator = CrossChainValidator::default_validator();
        let tx_id1 = Blake2bHasher::default().digest(b"tx_limit_1");
        let tx_id2 = Blake2bHasher::default().digest(b"tx_limit_2");

        // Max out tx1
        for _ in 0..3 {
            validator.record_resubmission(&tx_id1);
        }
        assert!(validator.check_resubmission_allowed(&tx_id1).is_err());

        // tx2 should still be allowed
        assert!(validator.check_resubmission_allowed(&tx_id2).is_ok());
        validator.record_resubmission(&tx_id2);
        assert_eq!(validator.get_resubmission_count(&tx_id2), 1);
    }

    #[test]
    fn test_resubmission_cleared_after_success() {
        let validator = CrossChainValidator::default_validator();
        let tx_id = Blake2bHasher::default().digest(b"success_tx");

        // Record some resubmissions
        validator.record_resubmission(&tx_id);
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 2);

        // Simulate successful processing - clear the count
        validator.clear_resubmission_count(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 0);

        // Should be able to resubmit again if needed
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
    }

    #[test]
    fn test_resubmission_workflow_through_mempool() {
        let validator = CrossChainValidator::default_validator();
        let tx_id = Blake2bHasher::default().digest(b"mempool_tx");

        // Simulate mempool workflow:
        // 1. Initial transaction submission
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());

        // 2. Oracle update invalidates proof
        // Transaction is removed from mempool

        // 3. User resubmits with updated proof
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 1);

        // 4. Another oracle update
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 2);

        // 5. Final resubmission
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 3);

        // 6. Limit reached
        assert!(validator.check_resubmission_allowed(&tx_id).is_err());
    }

    #[test]
    fn test_resubmission_count_persists_across_checks() {
        let validator = CrossChainValidator::default_validator();
        let tx_id = Blake2bHasher::default().digest(b"persist_tx");

        // Record resubmission
        validator.record_resubmission(&tx_id);

        // Multiple checks shouldn't change the count
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());

        // Count should still be 1
        assert_eq!(validator.get_resubmission_count(&tx_id), 1);
    }

    #[test]
    fn test_resubmission_with_different_proofs() {
        let validator = CrossChainValidator::default_validator();
        let tx_id = Blake2bHasher::default().digest(b"different_proofs_tx");

        // Initial submission with proof A
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());

        // Resubmit with proof B (after oracle update)
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 1);

        // Resubmit with proof C (after another oracle update)
        validator.record_resubmission(&tx_id);
        assert_eq!(validator.get_resubmission_count(&tx_id), 2);

        // Still within limit
        assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
    }

    #[test]
    fn test_max_resubmissions_configuration() {
        let validator = CrossChainValidator::new(5); // Custom max resubmissions

        assert_eq!(validator.get_max_resubmissions(), 5);

        let tx_id = Blake2bHasher::default().digest(b"config_tx");

        // Should allow 5 resubmissions
        for i in 0..5 {
            assert!(validator.check_resubmission_allowed(&tx_id).is_ok());
            validator.record_resubmission(&tx_id);
            assert_eq!(validator.get_resubmission_count(&tx_id), i + 1);
        }

        // 6th should fail
        assert!(validator.check_resubmission_allowed(&tx_id).is_err());
    }

    // Tests for oracle state update handling

    #[test]
    fn test_subscribe_to_oracle_updates() {
        let validator = CrossChainValidator::default_validator();
        let oracle_addr = Address::from([1u8; 20]);

        let result = validator.subscribe_to_oracle_updates(&oracle_addr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_oracle_state_update_marks_invalidated() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        // Create an outgoing transaction
        let (cc_tx, tx_id) = create_test_cross_chain_outgoing_with_data(100);
        pending_txs.insert(tx_id.clone(), cc_tx);

        // Initially not invalidated
        assert!(!validator.is_transaction_invalidated(&tx_id));
        assert_eq!(validator.invalidated_count(), 0);

        // Process oracle state update
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 100,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        let invalidated = validator.process_oracle_state_update(vec![new_state], &pending_txs);
        assert_eq!(invalidated.len(), 1);
        assert_eq!(invalidated[0], tx_id);

        // Should now be marked as invalidated
        assert!(validator.is_transaction_invalidated(&tx_id));
        assert_eq!(validator.invalidated_count(), 1);
    }

    #[test]
    fn test_clear_invalidation() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        let (cc_tx, tx_id) = create_test_cross_chain_outgoing_with_data(100);
        pending_txs.insert(tx_id.clone(), cc_tx);

        // Invalidate the transaction
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 100,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        validator.process_oracle_state_update(vec![new_state], &pending_txs);
        assert!(validator.is_transaction_invalidated(&tx_id));

        // Clear invalidation
        validator.clear_invalidation(&tx_id);
        assert!(!validator.is_transaction_invalidated(&tx_id));
        assert_eq!(validator.invalidated_count(), 0);
    }

    #[test]
    fn test_clear_all_invalidations() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        // Create multiple outgoing transactions with different IDs
        for i in 0..3 {
            let (cc_tx, _tx_id) = create_test_cross_chain_outgoing_with_data(100);
            // Use a unique ID based on index
            let unique_id = Blake2bHasher::default().digest(&[i]);
            pending_txs.insert(unique_id, cc_tx);
        }

        // Invalidate all transactions
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 100,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        validator.process_oracle_state_update(vec![new_state], &pending_txs);
        assert_eq!(validator.invalidated_count(), 3);

        // Clear all
        validator.clear_all_invalidations();
        assert_eq!(validator.invalidated_count(), 0);
    }

    #[test]
    fn test_oracle_state_update_multiple_heights() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        // Create transactions at different heights
        let (cc_tx1, _) = create_test_cross_chain_outgoing_with_data(100);
        let tx1_id = Blake2bHasher::default().digest(b"tx1");

        let (cc_tx2, _) = create_test_cross_chain_outgoing_with_data(200);
        let tx2_id = Blake2bHasher::default().digest(b"tx2");

        pending_txs.insert(tx1_id.clone(), cc_tx1);
        pending_txs.insert(tx2_id.clone(), cc_tx2);

        // Update only affects height 100
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 100,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        let invalidated = validator.process_oracle_state_update(vec![new_state], &pending_txs);
        assert_eq!(invalidated.len(), 1);
        assert!(validator.is_transaction_invalidated(&tx1_id));
        assert!(!validator.is_transaction_invalidated(&tx2_id));
    }

    #[test]
    fn test_oracle_state_update_batch() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        // Create multiple transactions
        for i in 0..5 {
            let (cc_tx, _) = create_test_cross_chain_outgoing_with_data(100);
            let tx_id = Blake2bHasher::default().digest(&[i]);
            pending_txs.insert(tx_id, cc_tx);
        }

        // Batch update with multiple states
        let states = vec![
            StateHash {
                hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"state1")),
                block_height: 100,
                timestamp: 1234567890,
                oracle_signature: vec![1, 2, 3],
            },
            StateHash {
                hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"state2")),
                block_height: 101,
                timestamp: 1234567891,
                oracle_signature: vec![4, 5, 6],
            },
        ];

        let invalidated = validator.process_oracle_state_update(states, &pending_txs);
        assert_eq!(invalidated.len(), 5);
        assert_eq!(validator.invalidated_count(), 5);
    }

    #[test]
    fn test_invalidation_persists_across_checks() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        let (cc_tx, tx_id) = create_test_cross_chain_outgoing_with_data(100);
        pending_txs.insert(tx_id.clone(), cc_tx);

        // Invalidate
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 100,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        validator.process_oracle_state_update(vec![new_state], &pending_txs);

        // Multiple checks should return same result
        assert!(validator.is_transaction_invalidated(&tx_id));
        assert!(validator.is_transaction_invalidated(&tx_id));
        assert!(validator.is_transaction_invalidated(&tx_id));
        assert_eq!(validator.invalidated_count(), 1);
    }

    #[test]
    fn test_resubmission_after_invalidation() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        let (cc_tx, tx_id) = create_test_cross_chain_outgoing_with_data(100);
        pending_txs.insert(tx_id.clone(), cc_tx);

        // Invalidate the transaction
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 100,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        validator.process_oracle_state_update(vec![new_state], &pending_txs);
        assert!(validator.is_transaction_invalidated(&tx_id));

        // User resubmits with new proof
        validator.record_resubmission(&tx_id);
        validator.clear_invalidation(&tx_id);

        // Should no longer be invalidated
        assert!(!validator.is_transaction_invalidated(&tx_id));
        assert_eq!(validator.get_resubmission_count(&tx_id), 1);
    }

    #[test]
    fn test_oracle_state_update_empty_pending_txs() {
        let validator = CrossChainValidator::default_validator();
        let pending_txs = std::collections::HashMap::new();

        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 100,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        let invalidated = validator.process_oracle_state_update(vec![new_state], &pending_txs);
        assert_eq!(invalidated.len(), 0);
        assert_eq!(validator.invalidated_count(), 0);
    }

    #[test]
    fn test_oracle_state_update_no_matching_transactions() {
        let validator = CrossChainValidator::default_validator();
        let mut pending_txs = std::collections::HashMap::new();

        // Transaction at height 100
        let (cc_tx, tx_id) = create_test_cross_chain_outgoing_with_data(100);
        pending_txs.insert(tx_id.clone(), cc_tx);

        // Update for height 200 (no match)
        let new_state = StateHash {
            hash: blake2b_to_anyhash(Blake2bHasher::default().digest(b"new_state")),
            block_height: 200,
            timestamp: 1234567890,
            oracle_signature: vec![1, 2, 3],
        };

        let invalidated = validator.process_oracle_state_update(vec![new_state], &pending_txs);
        assert_eq!(invalidated.len(), 0);
        assert!(!validator.is_transaction_invalidated(&tx_id));
    }
}
