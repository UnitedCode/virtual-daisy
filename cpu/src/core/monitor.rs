// Modified for Virtual Daisy's browser backend; see UPSTREAM.md.
//!
//! Cortex core register operations
//!

use crate::Processor;

///
/// Memory monitoring for atomic instructions (STREX, LDREX, ...)
///
pub trait Monitor {
    ///
    /// check if there has not been changes to given `address` with given bit `size` since
    /// last time monitors were set.
    ///
    /// Return
    ///  true if monitor pass (no changes to the given area), false otherwise.
    ///
    fn exclusive_monitors_pass(&mut self, address: u32, size: usize) -> bool;

    ///
    /// sets a monitor for load exclusive operation
    ///
    fn set_exclusive_monitors(&mut self, address: u32, size: usize);
}

impl Monitor for Processor {
    fn exclusive_monitors_pass(&mut self, address: u32, size: usize) -> bool {
        self.exclusive_monitor.take() == Some((address, size))
    }

    fn set_exclusive_monitors(&mut self, address: u32, size: usize) {
        self.exclusive_monitor = Some((address, size));
    }
}
