// Bridge contract module
//
// This module contains all bridge contract functionality for cross-chain asset transfers.
// The main implementation is in bridge_contract.rs, with transaction data structures in structs.rs

use nimiq_primitives::account::AccountType;
use nimiq_serde::Deserialize;

use crate::{
    account::AccountTransactionVerification, SignatureProof, Transaction, TransactionError,
    TransactionFlags,
};

// Module declarations
mod core;
pub mod decode_arithmetic;
pub mod structs;

// Re-export everything from core and structs
pub use self::{core::*, structs::*};

/// The verifier trait for a bridge contract. This only uses data available in the transaction.
pub struct BridgeContractVerifier {}

impl AccountTransactionVerification for BridgeContractVerifier {
    fn verify_incoming_transaction(transaction: &Transaction) -> Result<(), TransactionError> {
        assert_eq!(transaction.recipient_type, AccountType::Bridge);

        if transaction
            .flags
            .contains(TransactionFlags::CONTRACT_CREATION)
        {
            // Contract creation transaction
            if transaction.recipient != transaction.contract_creation_address() {
                warn!(
                    "Recipient address must match contract creation address for the following transaction:\n{:?}",
                    transaction
                );
                return Err(TransactionError::InvalidForRecipient);
            }

            let data = CreationTransactionData::parse(transaction)?;
            data.verify()?;
        } else if transaction.flags.contains(TransactionFlags::SIGNALING) {
            // Signaling transaction for cross-chain operations (outgoing from bridge)
            let data = OutgoingBridgeTransactionData::parse(transaction)?;
            data.verify(transaction)?;
        } else {
            warn!(
                "Only contract creation or signaling transactions are allowed for the following transaction:\n{:?}",
                transaction
            );
            return Err(TransactionError::InvalidForRecipient);
        }

        Ok(())
    }

    fn verify_outgoing_transaction(transaction: &Transaction) -> Result<(), TransactionError> {
        assert_eq!(transaction.sender_type, AccountType::Bridge);

        if !transaction.sender_data.is_empty() {
            warn!(
                "The following transaction can't have sender data:\n{:?}",
                transaction
            );
            return Err(TransactionError::Overflow);
        }

        // Verify signature.
        let signature_proof = SignatureProof::deserialize_all(&transaction.proof)?;

        if !signature_proof.verify(&transaction.serialize_content()) {
            warn!("Invalid signature for this transaction:\n{:?}", transaction);
            return Err(TransactionError::InvalidProof);
        }

        Ok(())
    }
}
