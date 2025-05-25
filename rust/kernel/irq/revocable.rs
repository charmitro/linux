// SPDX-License-Identifier: GPL-2.0

//! Revocable interrupt handler support.
//!
//! This module provides interrupt handlers that can be revoked at runtime,
//! useful for scenarios where handlers need to be dynamically disabled.

use crate::{
    irq::{IrqContext, IrqHandler, IrqReturn, ThreadContext, ThreadedIrqHandler},
    revocable::{Revocable, RevocableGuard},
    sync::Arc,
};

/// A revocable IRQ handler data wrapper.
///
/// This allows handler data to be revoked at runtime, causing subsequent
/// interrupts to return `IrqReturn::None`.
pub struct RevocableIrqData<T: Send + Sync> {
    inner: Arc<Revocable<T>>,
}

impl<T: Send + Sync> RevocableIrqData<T> {
    /// Get the Arc to the revocable data.
    pub fn arc(&self) -> &Arc<Revocable<T>> {
        &self.inner
    }

    /// Revoke access to the data.
    pub fn revoke(&self) {
        self.inner.revoke();
    }

    /// Try to access the inner data.
    pub fn try_access(&self) -> Option<RevocableGuard<'_, T>> {
        self.inner.try_access()
    }
}

impl<T: Send + Sync> Clone for RevocableIrqData<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: Send + Sync> From<Arc<Revocable<T>>> for RevocableIrqData<T> {
    fn from(inner: Arc<Revocable<T>>) -> Self {
        Self { inner }
    }
}

/// Handler that can be revoked at runtime.
///
/// This handler wraps data in a `Revocable` container, allowing
/// the data to be made inaccessible at runtime.
pub struct RevocableHandler<T: Send + Sync> {
    _phantom: core::marker::PhantomData<T>,
}

impl<T: Send + Sync> IrqHandler for RevocableHandler<T> {
    type Data = Arc<Revocable<T>>;

    fn handle_irq(data: &Self::Data, _ctx: &IrqContext<'_>) -> IrqReturn {
        if let Some(_data_ref) = data.try_access() {
            // In a real implementation, T would need to implement a trait
            // that provides the actual interrupt handling logic.
            // For now, we just return Handled if data is accessible.
            IrqReturn::Handled
        } else {
            // Data has been revoked
            IrqReturn::None
        }
    }
}

/// Threaded handler that can be revoked at runtime.
pub struct RevocableThreadedHandler<T: Send + Sync> {
    _phantom: core::marker::PhantomData<T>,
}

impl<T: Send + Sync> IrqHandler for RevocableThreadedHandler<T> {
    type Data = Arc<Revocable<T>>;

    fn handle_irq(data: &Self::Data, _ctx: &IrqContext<'_>) -> IrqReturn {
        if data.try_access().is_some() {
            // Request thread to handle
            IrqReturn::WakeThread
        } else {
            // Data has been revoked
            IrqReturn::None
        }
    }
}

impl<T: Send + Sync> ThreadedIrqHandler for RevocableThreadedHandler<T> {
    fn handle_thread(data: &Self::Data, _ctx: &ThreadContext<'_>) -> IrqReturn {
        if let Some(_data_ref) = data.try_access() {
            // In a real implementation, T would need to implement a trait
            // that provides the actual interrupt handling logic.
            IrqReturn::Handled
        } else {
            IrqReturn::None
        }
    }
}
