use nimiq_keys::Address;
#[cfg(feature = "interaction-traits")]
use nimiq_primitives::account::AccountType;
use nimiq_primitives::{account::AccountError, coin::Coin, transaction::TransactionError};
use nimiq_serde::{Deserialize, Serialize};
use nimiq_transaction::account::bridge_contract::ChainConfig;
#[cfg(feature = "interaction-traits")]
use nimiq_transaction::account::bridge_contract::{
    CreationTransactionData, OutgoingBridgeTransactionData,
};
#[cfg(feature = "interaction-traits")]
use nimiq_transaction::{inherent::Inherent, SignatureProof, Transaction};

use crate::{convert_receipt, AccountReceipt};
#[cfg(feature = "interaction-traits")]
use crate::{
    data_store::{DataStoreRead, DataStoreWrite},
    interaction_traits::{
        AccountInherentInteraction, AccountPruningInteraction, AccountTransactionInteraction,
    },
    reserved_balance::ReservedBalance,
    Account, BlockState, InherentLogger, Log, TransactionLog,
};

/// The Bridge contract for cross-chain asset transfers.
///
/// This contract manages cross-chain transactions by validating Merkle proofs
/// against oracle-verified state hashes and processing asset transfers between
/// different blockchain networks.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct BridgeContract {
    /// The owner/operator of the bridge contract
    pub owner: Address,

    /// The oracle contract address for state verification
    pub oracle_address: Address,

    /// Contract balance (deposits and fees)
    pub balance: Coin,

    /// Source chain ID this bridge instance supports
    pub source_chain_id: u32,

    /// Chain-specific configuration (address format, endianness, hash function, etc.)
    /// This is used to validate incoming transactions according to the source chain's rules
    pub chain_config: ChainConfig,

    /// Total number of cross-chain transactions processed
    pub transaction_count: u64,

    /// Highest processed nonce for each address (replay protection)
    /// Maps address (raw bytes) -> highest processed nonce
    pub processed_nonces: std::collections::HashMap<Vec<u8>, u64>,
}

#[cfg(feature = "interaction-traits")]
impl BridgeContract {
    fn can_change_balance(
        &self,
        transaction: &Transaction,
        new_balance: Coin,
        is_reserve: bool,
    ) -> Result<(), AccountError> {
        // Check transaction signer is contract owner.
        let signature_proof = SignatureProof::deserialize_all(&transaction.proof)?;

        if !signature_proof.is_signed_by(&self.owner) {
            return Err(AccountError::InvalidSignature);
        }

        // If withdrawing, must withdraw the full balance (contract deletion)
        if new_balance < self.balance {
            // For reserve_balance, we allow reserving any amount up to the full balance
            if is_reserve {
                return Ok(());
            }
            // For actual withdrawal, only allow full withdrawal (balance goes to zero)
            if new_balance != Coin::ZERO {
                return Err(AccountError::InvalidForSender);
            }
            // Check that the transaction value equals the current balance
            if transaction.value != self.balance {
                return Err(AccountError::InvalidForSender);
            }
        }

        Ok(())
    }
}

#[cfg(feature = "interaction-traits")]
fn validate_oracle_hash_compatibility(
    oracle_address: &Address,
    chain_config: &ChainConfig,
    data_store: DataStoreWrite,
) -> Result<(), AccountError> {
    use nimiq_primitives::key_nibbles::KeyNibbles;
    use nimiq_transaction::HashType;

    use crate::Account;

    // Convert address to KeyNibbles for data store query
    let oracle_key = KeyNibbles::from(oracle_address);

    // Query the oracle contract from the data store
    let oracle_account = data_store.get::<Account>(&oracle_key);

    // Extract oracle contract if it exists
    let oracle = match oracle_account {
        Some(Account::Oracle(oracle)) => oracle,
        Some(_) => {
            log::warn!(
                "Bridge creation failed: oracle_address {} is not an oracle contract",
                oracle_address
            );
            return Err(AccountError::InvalidForRecipient);
        }
        None => {
            log::warn!(
                "Bridge creation failed: oracle contract {} does not exist",
                oracle_address
            );
            return Err(AccountError::InvalidForRecipient);
        }
    };

    // If oracle has no hashes yet, any hash function is acceptable
    if oracle.hashes.is_empty() {
        log::info!(
            "Oracle {} is empty, bridge hash function {} will be accepted",
            oracle_address,
            HashType::from_any_hash(&chain_config.hash_function).name()
        );
        return Ok(());
    }

    // Get the oracle's hash type from its first hash
    let oracle_hash_type = HashType::from_any_hash(&oracle.hashes[0]);
    let bridge_hash_type = HashType::from_any_hash(&chain_config.hash_function);

    // Validate hash types match
    if oracle_hash_type != bridge_hash_type {
        log::warn!(
            "Bridge creation failed: hash function mismatch. \
             Oracle at {} uses {}, but bridge is configured for {}. \
             Please update the bridge's chain_config.hash_function to match the oracle.",
            oracle_address,
            oracle_hash_type.name(),
            bridge_hash_type.name()
        );
        return Err(AccountError::InvalidTransaction(
            TransactionError::InvalidData,
        ));
    }

    log::info!(
        "Bridge-oracle hash compatibility validated: both use {}",
        oracle_hash_type.name()
    );

    Ok(())
}

#[cfg(feature = "interaction-traits")]
impl AccountTransactionInteraction for BridgeContract {
    fn create_new_contract(
        transaction: &Transaction,
        initial_balance: Coin,
        _block_state: &BlockState,
        data_store: DataStoreWrite,
        tx_logger: &mut TransactionLog,
    ) -> Result<Account, AccountError> {
        let data = CreationTransactionData::parse(transaction)
            .map_err(AccountError::InvalidTransaction)?;

        // Verify the creation data
        data.verify().map_err(AccountError::InvalidTransaction)?;

        // Validate oracle hash compatibility
        validate_oracle_hash_compatibility(&data.oracle_address, &data.chain_config, data_store)?;

        // The deposit is the transaction value
        let deposit = transaction.value;

        tx_logger.push_log(Log::BridgeCreate {
            contract_address: transaction.recipient.clone(),
            owner: data.owner.clone(),
            oracle_address: data.oracle_address.clone(),
            source_chain_id: data.source_chain_id,
            deposit,
        });

        Ok(Account::Bridge(BridgeContract {
            balance: initial_balance + deposit,
            owner: data.owner,
            oracle_address: data.oracle_address,
            source_chain_id: data.source_chain_id,
            chain_config: data.chain_config,
            transaction_count: 0,
            processed_nonces: std::collections::HashMap::new(),
        }))
    }

    fn revert_new_contract(
        &mut self,
        transaction: &Transaction,
        _block_state: &BlockState,
        _data_store: DataStoreWrite,
        tx_logger: &mut TransactionLog,
    ) -> Result<(), AccountError> {
        self.balance -= transaction.value;

        tx_logger.push_log(Log::BridgeCreate {
            contract_address: transaction.recipient.clone(),
            owner: self.owner.clone(),
            oracle_address: self.oracle_address.clone(),
            source_chain_id: self.source_chain_id,
            deposit: transaction.value,
        });

        Ok(())
    }

    fn commit_incoming_transaction(
        &mut self,
        transaction: &Transaction,
        _block_state: &BlockState,
        _data_store: DataStoreWrite,
        tx_logger: &mut TransactionLog,
    ) -> Result<Option<AccountReceipt>, AccountError> {
        // Regular incoming transactions to bridge (user locking funds)
        // The transaction should have IncomingTransaction in recipient_data
        // specifying the target chain and address

        // Increment bridge balance (user is locking funds)
        self.balance += transaction.value;
        self.transaction_count += 1;

        tx_logger.push_log(Log::BridgeIncoming {
            contract_address: transaction.recipient.clone(),
            sender: transaction.sender.clone(),
            value: transaction.value,
        });

        Ok(None)
    }

    fn revert_incoming_transaction(
        &mut self,
        transaction: &Transaction,
        _block_state: &BlockState,
        _receipt: Option<AccountReceipt>,
        _data_store: DataStoreWrite,
        tx_logger: &mut TransactionLog,
    ) -> Result<(), AccountError> {
        // Revert the balance increase (user was locking funds)
        self.balance -= transaction.value;
        self.transaction_count -= 1;

        tx_logger.push_log(Log::BridgeIncoming {
            contract_address: transaction.recipient.clone(),
            sender: transaction.sender.clone(),
            value: transaction.value,
        });

        Ok(())
    }

    fn commit_outgoing_transaction(
        &mut self,
        transaction: &Transaction,
        _block_state: &BlockState,
        data_store: DataStoreWrite,
        tx_logger: &mut TransactionLog,
    ) -> Result<Option<AccountReceipt>, AccountError> {
        // Parse the outgoing bridge transaction data from recipient_data
        let outgoing_data = OutgoingBridgeTransactionData::parse(transaction)
            .map_err(AccountError::InvalidTransaction)?;

        // Verify owner signature
        if !outgoing_data.proof.is_signed_by(&self.owner) {
            return Err(AccountError::InvalidSignature);
        }

        // Verify the transaction signature
        outgoing_data
            .verify(transaction)
            .map_err(AccountError::InvalidTransaction)?;

        // Parse burn data using the validation program
        let parsed_burn = outgoing_data
            .burn_proof
            .parse_burn_data(&self.chain_config)
            .map_err(|e| {
                log::warn!("Failed to parse burn data: {:?}", e);
                AccountError::InvalidTransaction(TransactionError::InvalidData)
            })?;

        // Verify the target chain ID matches this bridge's source chain ID
        // (burn happened on source chain, releasing on Nimiq)
        if parsed_burn.target_chain_id != self.source_chain_id {
            log::warn!(
                "Chain ID mismatch: expected {}, got {}",
                self.source_chain_id,
                parsed_burn.target_chain_id
            );
            return Err(AccountError::InvalidTransaction(
                TransactionError::InvalidData,
            ));
        }

        // Check nonce for replay protection
        let highest_nonce = self
            .processed_nonces
            .get(&parsed_burn.target_address)
            .copied()
            .unwrap_or(0);

        if parsed_burn.target_nonce <= highest_nonce {
            log::warn!(
                "Nonce already used: {} <= {}",
                parsed_burn.target_nonce,
                highest_nonce
            );
            return Err(AccountError::InvalidTransaction(
                TransactionError::InvalidData,
            ));
        }

        // Verify merkle proof against oracle state hash
        use nimiq_primitives::key_nibbles::KeyNibbles;

        use crate::Account;

        // Query the oracle contract from the data store
        let oracle_key = KeyNibbles::from(&self.oracle_address);
        let oracle_account = data_store.get::<Account>(&oracle_key);

        let oracle = match oracle_account {
            Some(Account::Oracle(oracle)) => oracle,
            _ => {
                log::warn!(
                    "Oracle contract {} not found or invalid",
                    self.oracle_address
                );
                return Err(AccountError::InvalidForSender);
            }
        };

        // Verify the oracle state hash exists in the oracle's hashes
        let oracle_state_hash = &outgoing_data.burn_proof.oracle_state_hash;
        if !oracle.hashes.contains(oracle_state_hash) {
            log::warn!(
                "Oracle state hash not found in oracle contract: {:?}",
                oracle_state_hash
            );
            return Err(AccountError::InvalidTransaction(
                TransactionError::InvalidData,
            ));
        }

        // Compute the leaf hash from burn transaction data
        let leaf_hash = outgoing_data
            .burn_proof
            .extract_burn_transaction_hash(&self.chain_config.hash_function)
            .map_err(|e| {
                log::warn!("Failed to compute leaf hash: {:?}", e);
                AccountError::InvalidTransaction(TransactionError::InvalidData)
            })?;

        // Verify the merkle proof
        let proof_valid = outgoing_data
            .burn_proof
            .verify_merkle_proof(oracle_state_hash.clone(), leaf_hash)
            .map_err(|e| {
                log::warn!("Merkle proof verification failed: {:?}", e);
                AccountError::InvalidTransaction(TransactionError::InvalidProof)
            })?;

        if !proof_valid {
            log::warn!("Merkle proof verification returned false");
            return Err(AccountError::InvalidTransaction(
                TransactionError::InvalidProof,
            ));
        }

        // Decrement bridge balance (releasing funds)
        self.balance = self.balance.checked_sub(parsed_burn.amount).ok_or(
            AccountError::InsufficientFunds {
                needed: parsed_burn.amount,
                balance: self.balance,
            },
        )?;

        // Update processed nonces
        self.processed_nonces
            .insert(parsed_burn.target_address.clone(), parsed_burn.target_nonce);

        // Increment transaction count
        self.transaction_count += 1;

        // Log the outgoing transaction
        tx_logger.push_log(Log::BridgeOutgoing {
            contract_address: transaction.sender.clone(),
            recipient: Address::from(&parsed_burn.target_address[..]),
            value: parsed_burn.amount,
        });

        // Create receipt for revert
        let receipt = ProcessOutgoingReceipt {
            target_address: parsed_burn.target_address,
            nonce: parsed_burn.target_nonce,
            amount: parsed_burn.amount,
        };

        Ok(Some(receipt.into()))
    }

    fn revert_outgoing_transaction(
        &mut self,
        transaction: &Transaction,
        _block_state: &BlockState,
        receipt: Option<AccountReceipt>,
        _data_store: DataStoreWrite,
        tx_logger: &mut TransactionLog,
    ) -> Result<(), AccountError> {
        // Extract receipt
        let receipt = receipt.ok_or(AccountError::InvalidReceipt)?;
        let receipt = ProcessOutgoingReceipt::try_from(&receipt)?;

        // Revert balance change (add back the released funds)
        self.balance += receipt.amount;

        // Revert nonce update
        let current_nonce = self
            .processed_nonces
            .get(&receipt.target_address)
            .copied()
            .unwrap_or(0);

        if current_nonce == receipt.nonce {
            // Remove the nonce entry if it matches
            self.processed_nonces.remove(&receipt.target_address);
        } else {
            // If nonce doesn't match, restore the previous nonce
            // This handles the case where multiple transactions were processed
            if receipt.nonce > 0 {
                self.processed_nonces
                    .insert(receipt.target_address.clone(), receipt.nonce - 1);
            } else {
                self.processed_nonces.remove(&receipt.target_address);
            }
        }

        // Decrement transaction count
        self.transaction_count -= 1;

        // Log the revert
        tx_logger.push_log(Log::BridgeOutgoing {
            contract_address: transaction.sender.clone(),
            recipient: Address::from(&receipt.target_address[..]),
            value: receipt.amount,
        });

        Ok(())
    }

    fn commit_failed_transaction(
        &mut self,
        transaction: &Transaction,
        _block_state: &BlockState,
        _data_store: DataStoreWrite,
        tx_logger: &mut TransactionLog,
    ) -> Result<Option<AccountReceipt>, AccountError> {
        let new_balance = self.balance.safe_sub(transaction.fee)?;
        self.can_change_balance(transaction, new_balance, false)?;
        self.balance = new_balance;

        tx_logger.push_log(Log::pay_fee_log(transaction));

        Ok(None)
    }

    fn revert_failed_transaction(
        &mut self,
        transaction: &Transaction,
        _block_state: &BlockState,
        _receipt: Option<AccountReceipt>,
        _data_store: DataStoreWrite,
        tx_logger: &mut TransactionLog,
    ) -> Result<(), AccountError> {
        self.balance += transaction.fee;

        tx_logger.push_log(Log::pay_fee_log(transaction));

        Ok(())
    }

    fn reserve_balance(
        &self,
        transaction: &Transaction,
        reserved_balance: &mut ReservedBalance,
        _block_state: &BlockState,
        _data_store: DataStoreRead,
    ) -> Result<(), AccountError> {
        let needed = reserved_balance
            .balance()
            .checked_add(transaction.total_value())
            .ok_or(AccountError::InvalidCoinValue)?;
        let new_balance = self.balance.safe_sub(needed)?;
        self.can_change_balance(transaction, new_balance, true)?;

        reserved_balance.reserve(self.balance, transaction.total_value())
    }

    fn release_balance(
        &self,
        transaction: &Transaction,
        reserved_balance: &mut ReservedBalance,
        _data_store: DataStoreRead,
    ) -> Result<(), AccountError> {
        reserved_balance.release(transaction.total_value());
        Ok(())
    }
}

#[cfg(feature = "interaction-traits")]
impl AccountInherentInteraction for BridgeContract {
    fn commit_inherent(
        &mut self,
        _inherent: &Inherent,
        _block_state: &BlockState,
        _data_store: DataStoreWrite,
        _inherent_logger: &mut InherentLogger,
    ) -> Result<Option<AccountReceipt>, AccountError> {
        Err(AccountError::InvalidForTarget)
    }

    fn revert_inherent(
        &mut self,
        _inherent: &Inherent,
        _block_state: &BlockState,
        _receipt: Option<AccountReceipt>,
        _data_store: DataStoreWrite,
        _inherent_logger: &mut InherentLogger,
    ) -> Result<(), AccountError> {
        Err(AccountError::InvalidForTarget)
    }
}

#[cfg(feature = "interaction-traits")]
impl AccountPruningInteraction for BridgeContract {
    fn can_be_pruned(&self) -> bool {
        self.balance.is_zero()
    }

    fn prune(self, _data_store: DataStoreRead) -> Option<AccountReceipt> {
        Some(PrunedBridgeContract::from(self).into())
    }

    fn restore(
        _ty: AccountType,
        pruned_account: Option<&AccountReceipt>,
        _data_store: DataStoreWrite,
    ) -> Result<Account, AccountError> {
        let receipt = pruned_account.ok_or(AccountError::InvalidReceipt)?;
        let pruned_account = PrunedBridgeContract::try_from(receipt)?;
        Ok(Account::Bridge(BridgeContract::from(pruned_account)))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
struct PrunedBridgeContract {
    pub owner: Address,
    pub oracle_address: Address,
    pub source_chain_id: u32,
    pub chain_config: ChainConfig,
    pub transaction_count: u64,
    pub processed_nonces: std::collections::HashMap<Vec<u8>, u64>,
}

impl From<BridgeContract> for PrunedBridgeContract {
    fn from(contract: BridgeContract) -> Self {
        PrunedBridgeContract {
            owner: contract.owner,
            oracle_address: contract.oracle_address,
            source_chain_id: contract.source_chain_id,
            chain_config: contract.chain_config,
            transaction_count: contract.transaction_count,
            processed_nonces: contract.processed_nonces,
        }
    }
}

impl From<PrunedBridgeContract> for BridgeContract {
    fn from(receipt: PrunedBridgeContract) -> Self {
        BridgeContract {
            balance: Coin::ZERO,
            owner: receipt.owner,
            oracle_address: receipt.oracle_address,
            source_chain_id: receipt.source_chain_id,
            chain_config: receipt.chain_config,
            transaction_count: receipt.transaction_count,
            processed_nonces: receipt.processed_nonces,
        }
    }
}

convert_receipt!(PrunedBridgeContract);

/// Receipt for process outgoing transactions. This is necessary to be able to revert
/// these transactions.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
struct ProcessOutgoingReceipt {
    /// The address that received the transaction
    pub target_address: Vec<u8>,
    /// The nonce that was processed
    pub nonce: u64,
    /// The amount that was released
    pub amount: Coin,
}

convert_receipt!(ProcessOutgoingReceipt);
