// SPDX-License-Identifier: GPL-2.0

#include <linux/interrupt.h>
#include <linux/irq.h>

int rust_helper_request_irq(unsigned int irq, irq_handler_t handler,
                           unsigned long flags, const char *name, void *dev)
{
    return request_irq(irq, handler, flags, name, dev);
}

int rust_helper_request_threaded_irq(unsigned int irq, irq_handler_t handler,
                                    irq_handler_t thread_fn, unsigned long flags,
                                    const char *name, void *dev)
{
    return request_threaded_irq(irq, handler, thread_fn, flags, name, dev);
}

void rust_helper_free_irq(unsigned int irq, void *dev_id)
{
    free_irq(irq, dev_id);
}

void rust_helper_enable_irq(unsigned int irq)
{
    enable_irq(irq);
}

void rust_helper_disable_irq(unsigned int irq)
{
    disable_irq(irq);
}

void rust_helper_disable_irq_nosync(unsigned int irq)
{
    disable_irq_nosync(irq);
}

void rust_helper_local_irq_save(unsigned long *flags)
{
    local_irq_save(*flags);
}

void rust_helper_local_irq_restore(unsigned long flags)
{
    local_irq_restore(flags);
}

bool rust_helper_irqs_disabled(void)
{
    return irqs_disabled();
}

void rust_helper_raise_softirq(unsigned int nr)
{
    raise_softirq(nr);
}

void rust_helper___raise_softirq_irqoff(unsigned int nr)
{
    __raise_softirq_irqoff(nr);
}