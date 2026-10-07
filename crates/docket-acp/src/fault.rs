//! The errors we answer with. A session that does not exist and a session another app opened get
//! the same answer, as they do at the router.

use agent_client_protocol_schema::v1::Error;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn with(mut error: Error, message: &str) -> Error {
    error.message = message.to_owned();
    error
}

/// The parameters do not fit the method, or say something that is not so.
pub fn invalid(message: &str) -> Error {
    with(Error::invalid_params(), message)
}

/// The request is not allowed now.
pub fn not_now(message: &str) -> Error {
    with(Error::invalid_request(), message)
}

/// Something went wrong on our side; no detail leaves.
pub fn internal(message: &str) -> Error {
    with(Error::internal_error(), message)
}

/// No such session, as far as this editor may know.
pub fn unknown_session() -> Error {
    with(Error::resource_not_found(None), "no such session")
}

/// Reads a request's parameters as the schema crate's type for the method.
pub fn params<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|_| invalid("the parameters do not fit the method"))
}
