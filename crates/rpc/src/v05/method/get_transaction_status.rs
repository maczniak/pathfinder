use anyhow::Context;
use pathfinder_common::TransactionHash;
use serde_with::skip_serializing_none;
use starknet_gateway_types::pending::PendingData;

use crate::context::RpcContext;

#[derive(serde::Deserialize, Debug, PartialEq, Eq)]
pub struct GetTransactionStatusInput {
    transaction_hash: TransactionHash,
}

#[derive(serde::Serialize, Debug, PartialEq, Eq)]
#[skip_serializing_none]
pub struct GetTransactionStatusOutput {
    finality_status: FinalityStatus,
    /// Not present for received or rejected transactions.
    execution_status: Option<ExecutionStatus>,
}

#[derive(Copy, Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub enum FinalityStatus {
    #[serde(rename = "RECEIVED")]
    Received,
    #[serde(rename = "REJECTED")]
    Rejected,
    #[serde(rename = "ACCEPTED_ON_L1")]
    AcceptedOnL1,
    #[serde(rename = "ACCEPTED_ON_L2")]
    AcceptedOnL2,
}

impl TryFrom<starknet_gateway_types::reply::transaction_status::FinalityStatus> for FinalityStatus {
    type Error = anyhow::Error;

    fn try_from(
        value: starknet_gateway_types::reply::transaction_status::FinalityStatus,
    ) -> Result<Self, Self::Error> {
        use starknet_gateway_types::reply::transaction_status::FinalityStatus;
        match value {
            FinalityStatus::NotReceived => {
                Err(anyhow::anyhow!("Transaction not received by the gateway"))
            }
            FinalityStatus::Received => Ok(Self::Received),
            FinalityStatus::AcceptedOnL1 => Ok(Self::AcceptedOnL1),
            FinalityStatus::AcceptedOnL2 => Ok(Self::AcceptedOnL2),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionStatus {
    Succeeded,
    Reverted,
}

// Conversion for status from receipts.
impl From<starknet_gateway_types::reply::transaction::ExecutionStatus> for ExecutionStatus {
    fn from(value: starknet_gateway_types::reply::transaction::ExecutionStatus) -> Self {
        use starknet_gateway_types::reply::transaction::ExecutionStatus;
        match value {
            ExecutionStatus::Succeeded => Self::Succeeded,
            ExecutionStatus::Reverted => Self::Reverted,
        }
    }
}

crate::error::generate_rpc_error_subset!(GetTransactionStatusError: TxnHashNotFoundV04);

pub async fn get_transaction_status(
    context: RpcContext,
    input: GetTransactionStatusInput,
) -> Result<GetTransactionStatusOutput, GetTransactionStatusError> {
    // Check in pending block.
    if let Some(pending) = &context.pending_data {
        if let Some(status) = pending_status(pending, &input.transaction_hash).await {
            return Ok(status);
        }
    }

    // Check database.
    let span = tracing::Span::current();

    let db_status = tokio::task::spawn_blocking(move || {
        let _g = span.enter();

        let mut db = context
            .storage
            .connection()
            .context("Opening database connection")?;
        let db_tx = db.transaction().context("Creating database transaction")?;

        let Some((_, receipt, block_hash)) = db_tx
            .transaction_with_receipt(input.transaction_hash)
            .context("Fetching receipt from database")?
        else {
            return anyhow::Ok(None);
        };

        let l1_accepted = db_tx
            .block_is_l1_accepted(block_hash.into())
            .context("Quering block's status")?;

        let finality_status = if l1_accepted {
            FinalityStatus::AcceptedOnL1
        } else {
            FinalityStatus::AcceptedOnL2
        };

        Ok(Some(GetTransactionStatusOutput {
            finality_status,
            execution_status: Some(receipt.execution_status.into()),
        }))
    })
    .await
    .context("Joining database task")??;

    if let Some(db_status) = db_status {
        return Ok(db_status);
    }

    // Check gateway for rejected transactions.
    use starknet_gateway_client::GatewayApi;
    context
        .sequencer
        .transaction(input.transaction_hash)
        .await
        .context("Fetching transaction from gateway")
        .and_then(|tx| {
            use starknet_gateway_types::reply::transaction_status::FinalityStatus as GatewayFinalityStatus;
            use starknet_gateway_types::reply::transaction_status::ExecutionStatus as GatewayExecutionStatus;

            match (tx.finality_status, tx.execution_status) {
                (_, GatewayExecutionStatus::Rejected) => Ok(GetTransactionStatusOutput {
                    finality_status: FinalityStatus::Rejected,
                    execution_status: None,
                }),
                (GatewayFinalityStatus::Received, _) => Ok(GetTransactionStatusOutput {
                    finality_status: FinalityStatus::Received,
                    execution_status: None,
                }),
                (finality_status, GatewayExecutionStatus::Reverted) => Ok(GetTransactionStatusOutput {
                    finality_status: finality_status.try_into()?,
                    execution_status: Some(ExecutionStatus::Reverted),
                }),
                (finality_status, GatewayExecutionStatus::Succeeded) => Ok(GetTransactionStatusOutput {
                    finality_status: finality_status.try_into()?,
                    execution_status: Some(ExecutionStatus::Succeeded),
                }),
            }
        })
        .map_err(|_| GetTransactionStatusError::TxnHashNotFoundV04)
}

async fn pending_status(
    pending: &PendingData,
    tx_hash: &TransactionHash,
) -> Option<GetTransactionStatusOutput> {
    pending
        .block()
        .await
        .map(|block| {
            block.transaction_receipts.iter().find_map(|rx| {
                if &rx.transaction_hash == tx_hash {
                    Some(GetTransactionStatusOutput {
                        finality_status: FinalityStatus::AcceptedOnL2,
                        execution_status: Some(rx.execution_status.clone().into()),
                    })
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {

    use pathfinder_common::macro_prelude::*;

    use super::*;

    #[tokio::test]
    async fn l1_accepted() {
        let context = RpcContext::for_tests();
        // This transaction is in block 0 which is L1 accepted.
        let tx_hash = transaction_hash_bytes!(b"txn 0");
        let input = GetTransactionStatusInput {
            transaction_hash: tx_hash,
        };
        let status = get_transaction_status(context, input).await.unwrap();

        assert_eq!(
            status,
            GetTransactionStatusOutput {
                finality_status: FinalityStatus::AcceptedOnL1,
                execution_status: Some(ExecutionStatus::Succeeded)
            }
        );
    }

    #[tokio::test]
    async fn l2_accepted() {
        let context = RpcContext::for_tests();
        // This transaction is in block 1 which is not L1 accepted.
        let tx_hash = transaction_hash_bytes!(b"txn 1");
        let input = GetTransactionStatusInput {
            transaction_hash: tx_hash,
        };
        let status = get_transaction_status(context, input).await.unwrap();

        assert_eq!(
            status,
            GetTransactionStatusOutput {
                finality_status: FinalityStatus::AcceptedOnL2,
                execution_status: Some(ExecutionStatus::Succeeded)
            }
        );
    }

    #[tokio::test]
    async fn pending() {
        let context = RpcContext::for_tests_with_pending().await;
        let tx_hash = transaction_hash_bytes!(b"pending tx hash 0");
        let input = GetTransactionStatusInput {
            transaction_hash: tx_hash,
        };
        let status = get_transaction_status(context, input).await.unwrap();

        assert_eq!(
            status,
            GetTransactionStatusOutput {
                finality_status: FinalityStatus::AcceptedOnL2,
                execution_status: Some(ExecutionStatus::Succeeded)
            }
        );
    }

    #[tokio::test]
    async fn rejected() {
        let input = GetTransactionStatusInput {
            // Transaction hash known to be rejected by the testnet gateway.
            transaction_hash: transaction_hash!(
                "0x07c64b747bdb0831e7045925625bfa6309c422fded9527bacca91199a1c8d212"
            ),
        };
        let context = RpcContext::for_tests();
        let status = get_transaction_status(context, input).await.unwrap();

        assert_eq!(
            status,
            GetTransactionStatusOutput {
                finality_status: FinalityStatus::Rejected,
                execution_status: None,
            }
        );
    }

    #[tokio::test]
    async fn reverted() {
        let context = RpcContext::for_tests_with_pending().await;
        let input = GetTransactionStatusInput {
            transaction_hash: transaction_hash_bytes!(b"txn reverted"),
        };
        let status = get_transaction_status(context.clone(), input)
            .await
            .unwrap();
        assert_eq!(
            status,
            GetTransactionStatusOutput {
                finality_status: FinalityStatus::AcceptedOnL2,
                execution_status: Some(ExecutionStatus::Reverted),
            }
        );

        let input = GetTransactionStatusInput {
            transaction_hash: transaction_hash_bytes!(b"pending reverted"),
        };
        let status = get_transaction_status(context, input).await.unwrap();
        assert_eq!(
            status,
            GetTransactionStatusOutput {
                finality_status: FinalityStatus::AcceptedOnL2,
                execution_status: Some(ExecutionStatus::Reverted),
            }
        );
    }
}
