//! One enumeration of a device, with an expiry the caller supplies.

use crate::device::{AudioDevice, DeviceError, Snapshot};
use futures_util::future::{Either, select};
use std::future::Future;
use std::pin::pin;

/// How long one enumeration may take before the device is called absent.
pub const SNAPSHOT_BUDGET: std::time::Duration = std::time::Duration::from_secs(2);

/// One snapshot of `device`, or [`DeviceError::TimedOut`] if `expiry` completes first. The
/// expiry is a future the caller supplies, so a test fires it without waiting on any clock.
pub async fn snapshot_before<D: AudioDevice>(
    device: &D,
    expiry: impl Future<Output = ()>,
) -> Result<Snapshot, DeviceError> {
    let found = pin!(device.snapshot());
    match select(found, pin!(expiry)).await {
        Either::Left((found, _)) => found,
        Either::Right(((), _)) => Err(DeviceError::TimedOut),
    }
}
