use fake::Dummy;
use pathfinder_crypto::Felt;
use primitive_types::H160;

use crate::common::{Hash, Iteration};
use crate::{proto, proto_field, ToProtobuf, TryFromProtobuf};

#[derive(Debug, Clone, PartialEq, Eq, ToProtobuf, TryFromProtobuf, Dummy)]
#[protobuf(name = "crate::proto::receipt::MessageToL1")]
pub struct MessageToL1 {
    pub from_address: Felt,
    pub payload: Vec<Felt>,
    pub to_address: EthereumAddress,
}

// Avoid pathfinder_common dependency
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct EthereumAddress(pub H160);

#[derive(Debug, Clone, PartialEq, Eq, ToProtobuf, TryFromProtobuf, Dummy)]
#[protobuf(name = "crate::proto::receipt::receipt::ExecutionResources")]
pub struct ExecutionResources {
    pub builtins: execution_resources::BuiltinCounter,
    pub steps: u32,
    pub memory_holes: u32,
}

pub mod execution_resources {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, ToProtobuf, TryFromProtobuf, Dummy)]
    #[protobuf(name = "crate::proto::receipt::receipt::execution_resources::BuiltinCounter")]
    pub struct BuiltinCounter {
        pub bitwise: u32,
        pub ecdsa: u32,
        pub ec_op: u32,
        pub pedersen: u32,
        pub range_check: u32,
        pub poseidon: u32,
        pub keccak: u32,
        pub output: u32,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, ToProtobuf, TryFromProtobuf, Dummy)]
#[protobuf(name = "crate::proto::receipt::receipt::Common")]
pub struct ReceiptCommon {
    pub transaction_hash: Hash,
    pub actual_fee: Felt,
    pub messages_sent: Vec<MessageToL1>,
    pub execution_resources: ExecutionResources,
    pub revert_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, ToProtobuf, TryFromProtobuf, Dummy)]
#[protobuf(name = "crate::proto::receipt::receipt::Invoke")]
pub struct InvokeTransactionReceipt {
    pub common: ReceiptCommon,
}

#[derive(Debug, Clone, PartialEq, Eq, ToProtobuf, TryFromProtobuf, Dummy)]
#[protobuf(name = "crate::proto::receipt::receipt::L1Handler")]
pub struct L1HandlerTransactionReceipt {
    pub common: ReceiptCommon,
    pub msg_hash: Hash,
}

#[derive(Debug, Clone, PartialEq, Eq, ToProtobuf, TryFromProtobuf, Dummy)]
#[protobuf(name = "crate::proto::receipt::receipt::Declare")]
pub struct DeclareTransactionReceipt {
    pub common: ReceiptCommon,
}

#[derive(Debug, Clone, PartialEq, Eq, ToProtobuf, TryFromProtobuf, Dummy)]
#[protobuf(name = "crate::proto::receipt::receipt::Deploy")]
pub struct DeployTransactionReceipt {
    pub common: ReceiptCommon,
    pub contract_address: Felt,
}

#[derive(Debug, Clone, PartialEq, Eq, ToProtobuf, TryFromProtobuf, Dummy)]
#[protobuf(name = "crate::proto::receipt::receipt::DeployAccount")]
pub struct DeployAccountTransactionReceipt {
    pub common: ReceiptCommon,
    pub contract_address: Felt,
}

#[derive(Debug, Clone, PartialEq, Eq, Dummy)]

pub enum Receipt {
    Invoke(InvokeTransactionReceipt),
    Declare(DeclareTransactionReceipt),
    Deploy(DeployTransactionReceipt),
    DeployAccount(DeployAccountTransactionReceipt),
    L1Handler(L1HandlerTransactionReceipt),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ToProtobuf, TryFromProtobuf, Dummy)]
#[protobuf(name = "crate::proto::receipt::ReceiptsRequest")]
pub struct ReceiptsRequest {
    pub iteration: Iteration,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Dummy)]
pub enum ReceiptsResponse {
    Receipt(Receipt),
    #[default]
    Fin,
}

impl<T> Dummy<T> for EthereumAddress {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &T, rng: &mut R) -> Self {
        Self(H160::random_using(rng))
    }
}

impl ToProtobuf<proto::receipt::EthereumAddress> for EthereumAddress {
    fn to_protobuf(self) -> proto::receipt::EthereumAddress {
        proto::receipt::EthereumAddress {
            elements: self.0.to_fixed_bytes().into(),
        }
    }
}

impl TryFromProtobuf<proto::receipt::EthereumAddress> for EthereumAddress {
    fn try_from_protobuf(
        input: proto::receipt::EthereumAddress,
        field_name: &'static str,
    ) -> Result<Self, std::io::Error> {
        if input.elements.len() != primitive_types::H160::len_bytes() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid length for Ethereum address {field_name}"),
            ));
        }

        // from_slice() panics if the input length is incorrect, but we've already checked that
        let address = primitive_types::H160::from_slice(&input.elements);
        Ok(Self(address))
    }
}

impl TryFromProtobuf<proto::receipt::Receipt> for Receipt {
    fn try_from_protobuf(
        input: proto::receipt::Receipt,
        field_name: &'static str,
    ) -> Result<Self, std::io::Error> {
        use proto::receipt::receipt::Type::{
            Declare, DeployAccount, DeprecatedDeploy, Invoke, L1Handler,
        };

        Ok(match proto_field(input.r#type, field_name)? {
            Invoke(r) => Self::Invoke(TryFromProtobuf::try_from_protobuf(r, field_name)?),
            L1Handler(r) => Self::L1Handler(TryFromProtobuf::try_from_protobuf(r, field_name)?),
            Declare(r) => Self::Declare(TryFromProtobuf::try_from_protobuf(r, field_name)?),
            DeprecatedDeploy(r) => Self::Deploy(TryFromProtobuf::try_from_protobuf(r, field_name)?),
            DeployAccount(r) => {
                Self::DeployAccount(TryFromProtobuf::try_from_protobuf(r, field_name)?)
            }
        })
    }
}

impl ToProtobuf<proto::receipt::Receipt> for Receipt {
    fn to_protobuf(self) -> proto::receipt::Receipt {
        use proto::receipt::receipt::Type::{
            Declare, DeployAccount, DeprecatedDeploy, Invoke, L1Handler,
        };

        let r#type = Some(match self {
            Receipt::Invoke(r) => Invoke(r.to_protobuf()),
            Receipt::Declare(r) => Declare(r.to_protobuf()),
            Receipt::Deploy(r) => DeprecatedDeploy(r.to_protobuf()),
            Receipt::DeployAccount(r) => DeployAccount(r.to_protobuf()),
            Receipt::L1Handler(r) => L1Handler(r.to_protobuf()),
        });
        proto::receipt::Receipt { r#type }
    }
}

impl ToProtobuf<proto::receipt::ReceiptsResponse> for ReceiptsResponse {
    fn to_protobuf(self) -> proto::receipt::ReceiptsResponse {
        use proto::receipt::receipts_response::ReceiptMessage::{Fin, Receipt};
        proto::receipt::ReceiptsResponse {
            receipt_message: Some(match self {
                Self::Receipt(r) => Receipt(r.to_protobuf()),
                Self::Fin => Fin(proto::common::Fin {}),
            }),
        }
    }
}

impl TryFromProtobuf<proto::receipt::ReceiptsResponse> for ReceiptsResponse {
    fn try_from_protobuf(
        input: proto::receipt::ReceiptsResponse,
        field_name: &'static str,
    ) -> Result<Self, std::io::Error> {
        use proto::receipt::receipts_response::ReceiptMessage::{Fin, Receipt};
        Ok(match proto_field(input.receipt_message, field_name)? {
            Receipt(r) => Self::Receipt(TryFromProtobuf::try_from_protobuf(r, field_name)?),
            Fin(_) => Self::Fin,
        })
    }
}
