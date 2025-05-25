// SPDX-License-Identifier: GPL-2.0

//! Device-managed interrupt handling.
//!
//! This module provides device-managed (devres) wrappers for interrupt handling,
//! ensuring automatic cleanup when devices are unbound.

use crate::{
    bindings,
    device::Device,
    devres::Devres,
    error::{Error, Result},
    irq::{IrqFlags, IrqHandler, IrqRegistration, ThreadContext, ThreadedIrqHandler},
    prelude::*,
    str::CStr,
};
use core::pin::Pin;

/// Type alias for a device-managed IRQ registration.
type DevresIrqRegistration<T> =
    Devres<Pin<Box<IrqRegistration<T>, crate::alloc::allocator::Kmalloc>>>;

/// Type alias for a device-managed threaded IRQ registration.
type DevresThreadedIrqRegistration<T> =
    Devres<Pin<Box<ThreadedIrqRegistration<T>, crate::alloc::allocator::Kmalloc>>>;

/// Device extension for IRQ management.
impl Device {
    /// Request a device-managed IRQ.
    ///
    /// The IRQ will be automatically freed when the device is unbound.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `irq` is a valid IRQ number for this device.
    pub unsafe fn request_irq<T: IrqHandler>(
        &self,
        irq: u32,
        handler_data: T::Data,
        flags: IrqFlags,
        name: &CStr,
    ) -> Result<DevresIrqRegistration<T>> {
        // SAFETY: Caller guarantees IRQ is valid.
        let registration = Box::pin_init(
            unsafe { IrqRegistration::<T>::request(irq, handler_data, flags, name) },
            crate::alloc::flags::GFP_KERNEL,
        )?;

        Devres::new(self, registration, crate::alloc::flags::GFP_KERNEL)
    }

    /// Request a device-managed threaded IRQ.
    ///
    /// The IRQ will be automatically freed when the device is unbound.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `irq` is a valid IRQ number for this device.
    pub unsafe fn request_threaded_irq<T: ThreadedIrqHandler>(
        &self,
        irq: u32,
        handler_data: T::Data,
        flags: IrqFlags,
        name: &CStr,
    ) -> Result<DevresThreadedIrqRegistration<T>> {
        // SAFETY: Caller guarantees IRQ is valid.
        let registration = Box::pin_init(
            unsafe { ThreadedIrqRegistration::<T>::request(irq, handler_data, flags, name) },
            crate::alloc::flags::GFP_KERNEL,
        )?;

        Devres::new(self, registration, crate::alloc::flags::GFP_KERNEL)
    }
}

/// Registration for threaded interrupt handlers.
///
/// This type manages interrupts that have both a hard IRQ handler and a
/// threaded handler component.
#[pin_data(PinnedDrop)]
pub struct ThreadedIrqRegistration<T: ThreadedIrqHandler> {
    irq: u32,
    #[pin]
    handler_data: T::Data,
    dev_id: *mut core::ffi::c_void,
}

// SAFETY: ThreadedIrqRegistration can be transferred between threads.
unsafe impl<T: ThreadedIrqHandler> Send for ThreadedIrqRegistration<T> {}

// SAFETY: ThreadedIrqRegistration can be shared between threads.
unsafe impl<T: ThreadedIrqHandler> Sync for ThreadedIrqRegistration<T> {}

#[pinned_drop]
impl<T: ThreadedIrqHandler> PinnedDrop for ThreadedIrqRegistration<T> {
    fn drop(self: Pin<&mut Self>) {
        // SAFETY: We're in drop, so no more users of this IRQ.
        // The IRQ was successfully registered, so it's safe to free it.
        unsafe {
            bindings::free_irq(self.irq, self.dev_id);
        }
    }
}

impl<T: ThreadedIrqHandler> ThreadedIrqRegistration<T> {
    /// Request a threaded IRQ with the given handler.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `irq` is a valid IRQ number for the platform.
    pub unsafe fn request(
        irq: u32,
        handler_data: T::Data,
        flags: IrqFlags,
        name: &CStr,
    ) -> impl PinInit<Self> {
        let flags = flags.raw() as usize;
        let name = name.as_char_ptr();

        pin_init!(Self {
            irq,
            handler_data: handler_data,
            dev_id: core::ptr::null_mut(),
        })
        .pin_chain(move |slot| {
            // SAFETY: We're initializing dev_id to point to ourself.
            let slot_ptr = unsafe { slot.get_unchecked_mut() } as *mut Self;
            let dev_id = slot_ptr as *mut core::ffi::c_void;

            // SAFETY: Callbacks are valid function pointers,
            // and the caller guarantees irq is valid.
            let ret = unsafe {
                bindings::request_threaded_irq(
                    irq,
                    Some(irq_handler_callback::<T>),
                    Some(irq_thread_callback::<T>),
                    flags,
                    name,
                    dev_id,
                )
            };

            if ret < 0 {
                panic!(
                    "Failed to request threaded IRQ {}: {:?}",
                    irq,
                    Error::from_errno(ret)
                );
            }

            // SAFETY: Pointer is valid for lifetime of registration.
            unsafe {
                (*slot_ptr).dev_id = dev_id;
            }
            Ok(())
        })
    }

    /// Disable the IRQ.
    pub fn disable(&self) {
        // SAFETY: IRQ number is valid because we successfully registered it.
        unsafe {
            bindings::disable_irq(self.irq);
        }
    }

    /// Enable the IRQ.
    pub fn enable(&self) {
        // SAFETY: IRQ number is valid because we successfully registered it.
        unsafe {
            bindings::enable_irq(self.irq);
        }
    }

    /// Disable the IRQ without waiting for pending handlers.
    pub fn disable_nosync(&self) {
        // SAFETY: IRQ number is valid because we successfully registered it.
        unsafe {
            bindings::disable_irq_nosync(self.irq);
        }
    }

    /// Get the IRQ number.
    pub fn irq(&self) -> u32 {
        self.irq
    }
}

/// Adapter function for hard IRQ handler in threaded IRQs.
///
/// # Safety
///
/// - Must only be called by the kernel's IRQ subsystem
/// - `dev_id` must be a valid pointer to `ThreadedIrqRegistration<T>` created during registration
/// - Must be called in hard IRQ context
unsafe extern "C" fn irq_handler_callback<T: ThreadedIrqHandler>(
    _irq: core::ffi::c_int,
    dev_id: *mut core::ffi::c_void,
) -> core::ffi::c_uint {
    // SAFETY: dev_id is a valid pointer to ThreadedIrqRegistration<T> because
    // we passed it during registration and the kernel passes it back unchanged.
    let reg = unsafe { &*(dev_id as *const ThreadedIrqRegistration<T>) };

    // SAFETY: We're in IRQ context.
    let ctx = unsafe { crate::irq::IrqContext::new() };

    T::handle_irq(&reg.handler_data, &ctx) as _
}

/// Adapter function for threaded IRQ handler.
///
/// # Safety
///
/// - Must only be called by the kernel's IRQ subsystem
/// - `dev_id` must be a valid pointer to `ThreadedIrqRegistration<T>` created during registration
/// - Must be called in thread context by the kernel's IRQ thread
unsafe extern "C" fn irq_thread_callback<T: ThreadedIrqHandler>(
    _irq: core::ffi::c_int,
    dev_id: *mut core::ffi::c_void,
) -> core::ffi::c_uint {
    // SAFETY: dev_id is a valid pointer to ThreadedIrqRegistration<T> because
    // we passed it during registration and the kernel passes it back unchanged.
    let reg = unsafe { &*(dev_id as *const ThreadedIrqRegistration<T>) };

    let ctx = ThreadContext::new();
    T::handle_thread(&reg.handler_data, &ctx) as _
}
