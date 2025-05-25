// SPDX-License-Identifier: GPL-2.0

//! Interrupt handling abstractions.
//!
//! This module provides safe wrappers for Linux kernel interrupt handling,
//! including hard IRQ handlers, threaded IRQ handlers, and softirqs.
//!
//! # Examples
//!
//! ```no_run
//! # use kernel::prelude::*;
//! # use kernel::irq::{IrqHandler, IrqContext, IrqReturn};
//!
//! struct MyHandler;
//!
//! impl IrqHandler for MyHandler {
//!     type Data = u32;
//!     
//!     fn handle_irq(data: &Self::Data, _ctx: &IrqContext) -> IrqReturn {
//!         pr_info!("IRQ triggered with data: {}\n", data);
//!         IrqReturn::Handled
//!     }
//! }
//! ```

pub mod revocable;

use crate::{bindings, error::Error, prelude::*, str::CStr};
use core::{marker::PhantomData, pin::Pin};

/// IRQ return values indicating whether interrupt was handled.
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum IrqReturn {
    /// Interrupt was not from this device or not handled.
    None = bindings::irqreturn_IRQ_NONE,
    /// Interrupt was handled.
    Handled = bindings::irqreturn_IRQ_HANDLED,
    /// Handler requests to wake the handler thread (for threaded IRQs).
    WakeThread = bindings::irqreturn_IRQ_WAKE_THREAD,
}

/// Marker type for hard IRQ context where sleeping is forbidden.
///
/// This type is used as a parameter to interrupt handlers to ensure at
/// compile time that they do not attempt operations that might sleep.
pub struct IrqContext<'a> {
    _no_send_sync: PhantomData<*const ()>,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> IrqContext<'a> {
    /// Creates a new IRQ context marker.
    ///
    /// # Safety
    ///
    /// This should only be called from actual interrupt context.
    pub(crate) unsafe fn new() -> Self {
        Self {
            _no_send_sync: PhantomData,
            _phantom: PhantomData,
        }
    }
}

/// Marker type for threaded IRQ context where sleeping is allowed.
pub struct ThreadContext<'a> {
    _phantom: PhantomData<&'a ()>,
}

impl<'a> ThreadContext<'a> {
    /// Creates a new thread context marker.
    pub(crate) fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

/// Type-safe interrupt request flags.
#[derive(Debug, Copy, Clone)]
pub struct IrqFlags(core::ffi::c_ulong);

impl IrqFlags {
    /// Empty flags.
    pub const NONE: Self = Self(0);

    /// Allow sharing the IRQ among several devices.
    pub const SHARED: Self = Self(bindings::IRQF_SHARED as _);

    /// Interrupt is not reenabled after hardirq handler finished.
    pub const ONESHOT: Self = Self(bindings::IRQF_ONESHOT as _);

    /// Interrupt cannot be threaded.
    pub const NO_THREAD: Self = Self(bindings::IRQF_NO_THREAD as _);

    /// Add shared flag.
    pub const fn shared(self) -> Self {
        Self(self.0 | Self::SHARED.0)
    }

    /// Add oneshot flag.
    pub const fn oneshot(self) -> Self {
        Self(self.0 | Self::ONESHOT.0)
    }

    /// Get raw flags value.
    pub(crate) fn raw(self) -> core::ffi::c_ulong {
        self.0
    }
}

impl core::ops::BitOr for IrqFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for IrqFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Available softirq slots.
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SoftirqSlot {
    /// High priority softirq.
    Hi = bindings::HI_SOFTIRQ,
    /// Timer softirq.
    Timer = bindings::TIMER_SOFTIRQ,
    /// Network transmit softirq.
    NetTx = bindings::NET_TX_SOFTIRQ,
    /// Network receive softirq.
    NetRx = bindings::NET_RX_SOFTIRQ,
    /// Block layer softirq.
    Block = bindings::BLOCK_SOFTIRQ,
    /// IRQ poll softirq.
    IrqPoll = bindings::IRQ_POLL_SOFTIRQ,
    /// Tasklet softirq.
    Tasklet = bindings::TASKLET_SOFTIRQ,
    /// Scheduler softirq.
    Sched = bindings::SCHED_SOFTIRQ,
    /// High resolution timer softirq.
    HrTimer = bindings::HRTIMER_SOFTIRQ,
    /// RCU softirq.
    Rcu = bindings::RCU_SOFTIRQ,
}

/// Trait for interrupt handlers.
///
/// Implement this trait to define an interrupt handler that can be registered
/// with the kernel's interrupt subsystem.
pub trait IrqHandler: Send + Sync {
    /// Data type associated with this handler.
    type Data: Send + Sync;

    /// Called in hard IRQ context - cannot sleep or allocate.
    fn handle_irq(data: &Self::Data, ctx: &IrqContext<'_>) -> IrqReturn;
}

/// Trait for threaded interrupt handlers.
///
/// Threaded handlers run in process context and can sleep.
pub trait ThreadedIrqHandler: IrqHandler {
    /// Called in thread context - can sleep.
    ///
    /// This is called after `handle_irq` returns `IrqReturn::WakeThread`.
    fn handle_thread(data: &Self::Data, ctx: &ThreadContext<'_>) -> IrqReturn;
}

/// Trait for types that contain an IRQ handler.
///
/// This trait is used to allow types to specify which field contains
/// the handler data when multiple handlers might exist in the same type.
/// The `ID` parameter allows distinguishing between different handlers.
pub trait HasIrqHandler<T: IrqHandler, const ID: usize = 0> {
    /// Returns reference to the handler data.
    fn handler_data(&self) -> &T::Data;
}

/// Check if interrupts are currently disabled.
pub fn irqs_disabled() -> bool {
    // SAFETY: This is a simple check with no side effects.
    unsafe { bindings::irqs_disabled() }
}

/// Execute a closure with interrupts disabled.
///
/// This disables interrupts on the local CPU, executes the closure,
/// and then restores the previous interrupt state.
pub fn with_irqs_disabled<T, F: FnOnce() -> T>(f: F) -> T {
    let mut flags: usize = 0;

    // SAFETY: We're saving and restoring interrupt state correctly.
    unsafe {
        bindings::local_irq_save(&mut flags as *mut _);
    }

    let result = f();

    // SAFETY: Restoring previously saved flags.
    unsafe {
        bindings::local_irq_restore(flags);
    }

    result
}

/// Raise a softirq.
///
/// This schedules the softirq to run at the next opportunity.
pub fn raise_softirq(slot: SoftirqSlot) {
    // SAFETY: Valid softirq number from the enum.
    unsafe { bindings::raise_softirq(slot as _) }
}

/// Raise a softirq from IRQ context.
///
/// # Safety
///
/// This must only be called with interrupts disabled.
pub unsafe fn raise_softirq_irqoff(slot: SoftirqSlot) {
    // SAFETY: Caller ensures interrupts are disabled.
    unsafe { bindings::__raise_softirq_irqoff(slot as _) }
}

/// IRQ registration handle that automatically frees IRQ on drop.
///
/// This type ensures that registered interrupts are properly cleaned up
/// when the registration goes out of scope.
#[pin_data(PinnedDrop)]
pub struct IrqRegistration<T: IrqHandler> {
    irq: u32,
    #[pin]
    handler_data: T::Data,
    dev_id: *mut core::ffi::c_void,
}

// SAFETY: IrqRegistration can be transferred between threads.
unsafe impl<T: IrqHandler> Send for IrqRegistration<T> {}

// SAFETY: IrqRegistration can be shared between threads.
unsafe impl<T: IrqHandler> Sync for IrqRegistration<T> {}

#[pinned_drop]
impl<T: IrqHandler> PinnedDrop for IrqRegistration<T> {
    fn drop(self: Pin<&mut Self>) {
        // SAFETY: We're in drop, so no more users of this IRQ.
        // The IRQ was successfully registered, so it's safe to free it.
        unsafe {
            bindings::free_irq(self.irq, self.dev_id);
        }
    }
}

impl<T: IrqHandler> IrqRegistration<T> {
    /// Request an IRQ with the given handler.
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
            // We need to get the raw pointer from the pinned reference.
            let slot_ptr = unsafe { slot.get_unchecked_mut() } as *mut Self;
            let dev_id = slot_ptr as *mut core::ffi::c_void;

            // SAFETY: irq_handler_callback is a valid function pointer,
            // and the caller guarantees irq is valid.
            let ret = unsafe {
                bindings::request_irq(irq, Some(irq_handler_callback::<T>), flags, name, dev_id)
            };

            if ret < 0 {
                // We can't return an error from pin_chain in a way that works
                // with the current pin_init macro, so we panic on failure.
                // In practice, this should be wrapped by a higher-level API
                // that does proper error checking.
                panic!(
                    "Failed to request IRQ {}: {:?}",
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

/// Adapter function called from C interrupt handler.
///
/// This function is called by the kernel when an interrupt occurs.
/// It converts the C calling convention to the Rust trait method.
///
/// # Safety
///
/// - Must only be called by the kernel's IRQ subsystem
/// - `dev_id` must be a valid pointer to `IrqRegistration<T>` created during registration
/// - Must be called in hard IRQ context
unsafe extern "C" fn irq_handler_callback<T: IrqHandler>(
    _irq: core::ffi::c_int,
    dev_id: *mut core::ffi::c_void,
) -> core::ffi::c_uint {
    // SAFETY: dev_id is a valid pointer to IrqRegistration<T> because
    // we passed it during registration and the kernel passes it back unchanged.
    let reg = unsafe { &*(dev_id as *const IrqRegistration<T>) };

    // SAFETY: We're in IRQ context.
    let ctx = unsafe { IrqContext::new() };

    T::handle_irq(&reg.handler_data, &ctx) as _
}
