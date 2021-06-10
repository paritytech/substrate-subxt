// Copyright 2019-2021 Parity Technologies (UK) Ltd.
// This file is part of substrate-subxt.
//
// subxt is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// subxt is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with substrate-subxt.  If not, see <http://www.gnu.org/licenses/>.

use jsonrpsee_ws_client::Error as RequestError;
use sp_core::crypto::SecretStringError;
use sp_runtime::{
    transaction_validity::TransactionValidityError,
    DispatchError, TokenError, ArithmeticError,
};
use thiserror::Error;

use crate::metadata::{
    Metadata,
    MetadataError,
};

/// Error enum.
#[derive(Debug, Error)]
pub enum Error {
    /// Io error.
    #[error("Io error: {0}")]
    Io(#[from] std::io::Error),
    /// Codec error.
    #[error("Scale codec error: {0}")]
    Codec(#[from] codec::Error),
    /// Rpc error.
    #[error("Rpc error: {0}")]
    Rpc(#[from] RequestError),
    /// Serde serialization error
    #[error("Serde json error: {0}")]
    Serialization(#[from] serde_json::error::Error),
    /// Secret string error.
    #[error("Secret String Error")]
    SecretString(SecretStringError),
    /// Extrinsic validity error
    #[error("Transaction Validity Error: {0:?}")]
    Invalid(TransactionValidityError),
    /// Metadata error.
    #[error("Metadata error: {0}")]
    Metadata(#[from] MetadataError),
    /// Unregistered type sizes.
    #[error(
        "The following types do not have a type size registered: \
            {0:?} \
         Use `ClientBuilder::register_type_size` to register missing type sizes."
    )]
    MissingTypeSizes(Vec<String>),
    /// Type size unavailable.
    #[error("Type size unavailable while decoding event: {0:?}")]
    TypeSizeUnavailable(String),
    /// Runtime error.
    #[error("Runtime error: {0}")]
    Runtime(#[from] RuntimeError),
    /// Other error.
    #[error("Other error: {0}")]
    Other(String),
}

impl From<SecretStringError> for Error {
    fn from(error: SecretStringError) -> Self {
        Error::SecretString(error)
    }
}

impl From<TransactionValidityError> for Error {
    fn from(error: TransactionValidityError) -> Self {
        Error::Invalid(error)
    }
}

impl From<&str> for Error {
    fn from(error: &str) -> Self {
        Error::Other(error.into())
    }
}

impl From<String> for Error {
    fn from(error: String) -> Self {
        Error::Other(error)
    }
}

/// Runtime error.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RuntimeError {
    /// Module error.
    #[error("Runtime module error: {0}")]
    Module(ModuleError),
    /// At least one consumer is remaining so the account cannot be destroyed.
    #[error("At least one consumer is remaining so the account cannot be destroyed.")]
    ConsumerRemaining,
    /// There are no providers so the account cannot be created.
    #[error("There are no providers so the account cannot be created.")]
    NoProviders,
    /// Bad origin.
    #[error("Bad origin: throw by ensure_signed, ensure_root or ensure_none.")]
    BadOrigin,
    /// Cannot lookup.
    #[error("Cannot lookup some information required to validate the transaction.")]
    CannotLookup,
    /// An error to do with tokens.
    #[error("An error to do with tokens. {0:?}")]
    Token(TokenError),
    /// An arithmetic error.
    #[error("An arithmetic error. {0:?}")]
    Arithmetic(ArithmeticError),
    /// Other error.
    #[error("Other error: {0}")]
    Other(String),
}

impl RuntimeError {
    /// Converts a `DispatchError` into a subxt error.
    pub fn from_dispatch(
        metadata: &Metadata,
        error: DispatchError,
    ) -> Result<Self, Error> {
        match error {
            DispatchError::Module {
                index,
                error,
                message: _,
            } => {
                let module = metadata.module_with_errors(index)?;
                let error = module.error(error)?;
                Ok(Self::Module(ModuleError {
                    module: module.name().to_string(),
                    error: error.to_string(),
                }))
            },
            DispatchError::Token(err) => Ok(Self::Token(err)),
            DispatchError::Arithmetic(err) => Ok(Self::Arithmetic(err)),
            DispatchError::BadOrigin => Ok(Self::BadOrigin),
            DispatchError::CannotLookup => Ok(Self::CannotLookup),
            DispatchError::ConsumerRemaining => Ok(Self::ConsumerRemaining),
            DispatchError::NoProviders => Ok(Self::NoProviders),
            DispatchError::Other(msg) => Ok(Self::Other(msg.into())),
        }
    }
}

/// Module error.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{error} from {module}")]
pub struct ModuleError {
    /// The module where the error originated.
    pub module: String,
    /// The actual error code.
    pub error: String,
}
