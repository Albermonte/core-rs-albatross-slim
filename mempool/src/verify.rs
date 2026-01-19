use std::sync::Arc;

use nimiq_blockchain::Blockchain;
use nimiq_blockchain_interface::AbstractBlockchain;
use nimiq_hash::{Blake2bHash, Hash};
use nimiq_primitives::{
    account::{AccountError, AccountType},
    networks::NetworkId,
    transaction::TransactionError,
};
use nimiq_serde::Deserialize;
use nimiq_transaction::Transaction;
use parking_lot::RwLock;
use thiserror::Error;

use crate::{
    cross_chain::{CrossChainTransaction, CrossChainValidator},
    filter::MempoolFilter,
    mempool_state::MempoolState,
    mempool_transactions::TxPriority,
};

/// Error codes for the transaction verification
#[derive(Error, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum VerifyErr {
    #[error("Transaction is invalid: {0}")]
    InvalidTransaction(#[from] TransactionError),
    #[error("Transaction already included in chain")]
    AlreadyIncluded,
    #[error("Transaction not valid at current block number")]
    InvalidBlockNumber,
    #[error("Transaction cannot be applied to sender account: {0}")]
    InvalidAccount(#[from] AccountError),
    #[error("Transaction already in mempool")]
    Known,
    #[error("Transaction is filtered")]
    Filtered,
    #[error("Can't verify transaction without consensus")]
    NoConsensus,
    #[error("Cross-chain validation failed: {0}")]
    CrossChainValidationFailed(String),
}

/// Verifies a transaction and adds it to the mempool.
pub(crate) fn verify_tx(
    mut transaction: Transaction,
    blockchain: Arc<RwLock<Blockchain>>,
    network_id: NetworkId,
    mempool_state: &Arc<RwLock<MempoolState>>,
    filter: Arc<RwLock<MempoolFilter>>,
    cross_chain_validator: Option<Arc<RwLock<CrossChainValidator>>>,
    priority: TxPriority,
) -> Result<(), VerifyErr> {
    // 1. Verify transaction signature (and other stuff)
    transaction.verify_mut(network_id)?;

    // 2. Acquire blockchain read lock
    let blockchain = blockchain.read();

    // 3. Check validity window and already included
    let block_number = blockchain.block_number() + 1;
    if !transaction.is_valid_at(block_number) {
        return Err(VerifyErr::InvalidBlockNumber);
    }

    let hash: Blake2bHash = transaction.hash();
    if blockchain.contains_tx_in_validity_window(&hash.clone().into(), None) {
        return Err(VerifyErr::AlreadyIncluded);
    }

    // 4. Acquire the mempool state write lock
    let mut mempool_state = mempool_state.write();

    // 5. Check if we already know the transaction
    if mempool_state.contains(&hash) {
        // We already know this transaction, no need to process
        return Err(VerifyErr::Known);
    }

    // 6. Check if the transaction is going to be filtered.
    {
        let filter = filter.read();
        if !filter.accepts_transaction(&transaction) || filter.blacklisted(&hash) {
            // FIXME add transaction to blacklist
            return Err(VerifyErr::Filtered);
        }

        // TODO We also need to check:
        //  - filter.accepts_sender_balance()
        //  - filter.accepts_recipient_balance()
    }

    // 7. NEW: Validate cross-chain transactions if validator is configured
    if let Some(ref validator) = cross_chain_validator {
        // Check if this is a bridge transaction
        if is_bridge_transaction(&transaction) {
            log::debug!(
                "Detected bridge transaction: {:?}",
                transaction.hash::<Blake2bHash>()
            );

            // Query the bridge account from blockchain state to get oracle address
            match get_bridge_account(&transaction, &blockchain) {
                Ok(bridge_account) => {
                    log::debug!(
                        "Bridge account found: oracle={:?}, chain_id={}",
                        bridge_account.oracle_address,
                        bridge_account.source_chain_id
                    );

                    // Extract cross-chain data from transaction
                    match extract_cross_chain_transaction(
                        &transaction,
                        &bridge_account.chain_config,
                    ) {
                        Ok(cc_tx) => {
                            // Validate using the bridge's configuration and oracle address
                            validator
                                .read()
                                .validate_cross_chain_transaction(
                                    &cc_tx,
                                    &bridge_account.oracle_address,
                                    &bridge_account.chain_config,
                                    &blockchain,
                                )
                                .map_err(|e| {
                                    log::warn!("Cross-chain validation failed: {:?}", e);
                                    VerifyErr::CrossChainValidationFailed(format!("{:?}", e))
                                })?;

                            log::info!(
                                "Cross-chain transaction validated successfully: {:?}",
                                transaction.hash::<Blake2bHash>()
                            );
                        }
                        Err(e) => {
                            log::warn!("Failed to extract cross-chain data: {:?}", e);
                            return Err(e);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to get bridge account: {:?}", e);
                    return Err(e);
                }
            }
        }
    }

    // 8. Add transaction to the mempool. Balance checks are performed within put().
    mempool_state.put(&blockchain, transaction, priority)?;

    Ok(())
}

/// Checks if a transaction is a bridge transaction.
///
/// A transaction is considered a bridge transaction if:
/// - The recipient account type is Bridge
/// - The transaction contains cross-chain data in recipient_data
fn is_bridge_transaction(tx: &Transaction) -> bool {
    tx.recipient_type == AccountType::Bridge && !tx.recipient_data.is_empty()
}

/// Queries the bridge account from blockchain state.
///
/// This retrieves the bridge contract account which contains:
/// - oracle_address: The oracle for this bridge
/// - source_chain_id: Which chain this bridge handles
/// - chain_config: Chain-specific validation rules
fn get_bridge_account(
    tx: &Transaction,
    blockchain: &Blockchain,
) -> Result<nimiq_account::BridgeContract, VerifyErr> {
    use nimiq_account::Account;

    // Query the account at the recipient address
    let account = blockchain
        .get_account_if_complete(&tx.recipient)
        .ok_or_else(|| {
            VerifyErr::CrossChainValidationFailed("Bridge account not found".to_string())
        })?;

    // Ensure it's actually a bridge account
    match account {
        Account::Bridge(bridge) => Ok(bridge),
        _ => Err(VerifyErr::CrossChainValidationFailed(
            "Recipient is not a bridge account".to_string(),
        )),
    }
}

/// Extracts cross-chain transaction data from a regular transaction.
///
/// This function parses the recipient_data field to extract either an
/// IncomingTransaction or OutgoingTransaction based on the data format.
///
/// # Transaction Data Format
///
/// The recipient_data field contains serialized cross-chain transaction data:
/// - First byte indicates transaction type (0 = Incoming, 1 = Outgoing)
/// - Remaining bytes contain the serialized transaction data
fn extract_cross_chain_transaction(
    tx: &Transaction,
    chain_config: &nimiq_transaction::account::bridge_contract::ChainConfig,
) -> Result<CrossChainTransaction, VerifyErr> {
    use nimiq_transaction::{IncomingTransaction, OutgoingTransaction};

    if tx.recipient_data.is_empty() {
        return Err(VerifyErr::CrossChainValidationFailed(
            "Empty recipient data for bridge transaction".to_string(),
        ));
    }

    // First byte indicates transaction type
    let tx_type = tx.recipient_data[0];
    let data = &tx.recipient_data[1..];

    match tx_type {
        0 => {
            // Incoming transaction
            let incoming_tx = IncomingTransaction::deserialize_from_vec(data).map_err(|e| {
                VerifyErr::CrossChainValidationFailed(format!(
                    "Failed to deserialize incoming transaction: {:?}",
                    e
                ))
            })?;
            Ok(CrossChainTransaction::Incoming(incoming_tx))
        }
        1 => {
            // Outgoing transaction
            let outgoing_tx = OutgoingTransaction::deserialize_from_vec(data).map_err(|e| {
                VerifyErr::CrossChainValidationFailed(format!(
                    "Failed to deserialize outgoing transaction: {:?}",
                    e
                ))
            })?;
            CrossChainTransaction::new_outgoing(outgoing_tx, chain_config).map_err(|e| {
                VerifyErr::CrossChainValidationFailed(format!(
                    "Failed to parse outgoing transaction: {:?}",
                    e
                ))
            })
        }
        _ => Err(VerifyErr::CrossChainValidationFailed(format!(
            "Unknown cross-chain transaction type: {}",
            tx_type
        ))),
    }
}
