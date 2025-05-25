// SPDX-License-Identifier: GPL-2.0

//! Rust IRQ driver sample.

use kernel::{
    c_str,
    device::irq::DevresIrqRegistration,
    device::Core,
    irq::{IrqContext, IrqHandler, IrqReturn},
    of, platform,
    prelude::*,
    sync::Arc,
    types::ARef,
};

/// Sample IRQ handler that just counts interrupts
struct SampleIrqHandler;

impl IrqHandler for SampleIrqHandler {
    type Data = Arc<core::sync::atomic::AtomicU64>;

    fn handle_irq(data: &Self::Data, _ctx: &IrqContext<'_>) -> IrqReturn {
        // In hard IRQ context - cannot sleep
        // Just increment a counter to show we're receiving interrupts
        data.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        pr_info!(
            "PEOS! {}\n",
            data.load(core::sync::atomic::Ordering::Relaxed)
        );
        IrqReturn::Handled
    }
}

struct SampleIRQDriver {
    pdev: ARef<platform::Device>,
    // Store the IRQ registration to keep it alive
    _irq: Option<DevresIrqRegistration<SampleIrqHandler>>,
    // Keep the counter so we can print it later
    irq_count: Arc<core::sync::atomic::AtomicU64>,
}

struct Info(u32);

kernel::of_device_table!(
    OF_TABLE,
    MODULE_OF_TABLE,
    <SampleIRQDriver as platform::Driver>::IdInfo,
    [(of::DeviceId::new(c_str!("test,rust-device-irq")), Info(42))]
);

impl platform::Driver for SampleIRQDriver {
    type IdInfo = Info;
    const OF_ID_TABLE: Option<of::IdTable<Self::IdInfo>> = Some(&OF_TABLE);

    fn probe(
        pdev: &platform::Device<Core>,
        info: Option<&Self::IdInfo>,
    ) -> Result<Pin<KBox<Self>>> {
        dev_info!(pdev.as_ref(), "Rust IRQ sample driver probing\n");

        if let Some(info) = info {
            dev_info!(pdev.as_ref(), "Probed with info: '{}'.\n", info.0);
        }

        // Create the interrupt counter
        let irq_count = Arc::new(core::sync::atomic::AtomicU64::new(0), GFP_KERNEL)?;

        // Try to get IRQ from platform device (usually from device tree)
        // For this PoC, we'll use a hardcoded IRQ number if platform doesn't provide one
        // Using IRQ 11 which is often a free/shareable IRQ on x86
        const SAMPLE_IRQ: u32 = 0; // Example IRQ number

        // Register the IRQ
        // SAFETY: SAMPLE_IRQ is a valid IRQ number for this platform
        let irq_reg = unsafe {
            pdev.as_ref().request_irq::<SampleIrqHandler>(
                SAMPLE_IRQ,
                irq_count.clone(),
                kernel::irq::IrqFlags::SHARED,
                c_str!("rust_irq_sample"),
            )
        };

        let irq = match irq_reg {
            Ok(reg) => {
                dev_info!(
                    pdev.as_ref(),
                    "Successfully registered IRQ {}\n",
                    SAMPLE_IRQ
                );
                Some(reg)
            }
            Err(e) => {
                dev_warn!(
                    pdev.as_ref(),
                    "Failed to register IRQ {}: {:?}\n",
                    SAMPLE_IRQ,
                    e
                );
                None
            }
        };

        let drvdata = KBox::new(
            Self {
                pdev: pdev.into(),
                _irq: irq,
                irq_count,
            },
            GFP_KERNEL,
        )?;

        Ok(drvdata.into())
    }
}

impl Drop for SampleIRQDriver {
    fn drop(&mut self) {
        let count = self.irq_count.load(core::sync::atomic::Ordering::Relaxed);
        dev_info!(
            self.pdev.as_ref(),
            "Rust IRQ driver removed - handled {} interrupts\n",
            count
        );
    }
}

kernel::module_platform_driver! {
    type: SampleIRQDriver,
    name: "rust_driver_irq",
    authors: ["Charalampos Mitrodimas <charmitro@posteo.net"],
    description: "Rust IRQ sample driver",
    license: "GPL v2",
}
