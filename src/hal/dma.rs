//! An in-flight DMA operation owns its configured channel and payload.
//! Dropping it synchronously stops the channel before freeing the buffer.
//! Forgetting it retains both resources.

use crate::compat::Box;
use core::{future::Future, pin::Pin, task::{Context, Poll}};

/// Board implementation for one exclusively owned, preconfigured peripheral
/// DMA channel. The channel configuration determines the transfer direction.
///
/// # Safety
/// The channel must retain exclusive hardware ownership across moves and even
/// if forgotten. Hardware may access only the supplied byte range. Its DMA
/// memory mapping, cache maintenance, interrupt synchronization, and barriers
/// are the backend's responsibility. Reject an unsuitable buffer before use.
/// stop must synchronously quiesce all accesses and unregister any waker,
/// including after a partially failed start. It must not unwind.
pub unsafe trait DmaChannel: Sized + Unpin {
    type Error;

    /// # Safety
    /// The caller must reserve these bytes exclusively until stop completes.
    unsafe fn start(&mut self, buffer: *mut u8, len: usize) -> Result<(), Self::Error>;

    /// Register the latest waker and check completion without losing an IRQ.
    fn poll_complete(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
    fn stop(&mut self);

    /// Start on first poll. Empty payloads complete without programming DMA.
    fn transfer(self, buffer: Box<[u8]>) -> DmaTransfer<Self> {
        DmaTransfer { resources: Some((self, buffer)), started: false }
    }
}

/// Both resources are returned on completion, including hardware errors.
pub struct DmaCompletion<C: DmaChannel> {
    pub channel: C,
    pub buffer: Box<[u8]>,
    pub result: Result<(), C::Error>,
}

#[must_use = "await the transfer to drive it to completion"]
pub struct DmaTransfer<C: DmaChannel> {
    resources: Option<(C, Box<[u8]>)>,
    started: bool,
}

impl<C: DmaChannel> DmaTransfer<C> {
    fn stop(&mut self) {
        if self.started {
            self.resources.as_mut().expect("active DMA channel").0.stop();
            self.started = false;
        }
    }

    /// Stop a pending operation and recover the same channel and allocation.
    pub fn cancel(mut self) -> (C, Box<[u8]>) {
        self.stop();
        self.resources.take().expect("transfer already completed")
    }
}

impl<C: DmaChannel> Future for DmaTransfer<C> {
    type Output = DmaCompletion<C>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let (channel, buffer) = this.resources.as_mut().expect("transfer already completed");
        let result = if buffer.is_empty() {
            Ok(())
        } else {
            let start = if this.started {
                Ok(())
            } else {
                this.started = true;
                // The Box allocation stays fixed until the channel is stopped.
                unsafe { channel.start(buffer.as_mut_ptr(), buffer.len()) }
            };
            match start {
                Ok(()) => core::task::ready!(channel.poll_complete(cx)),
                Err(error) => Err(error),
            }
        };
        this.stop();
        let (channel, buffer) = this.resources.take().unwrap();
        Poll::Ready(DmaCompletion { channel, buffer, result })
    }
}

impl<C: DmaChannel> Drop for DmaTransfer<C> {
    fn drop(&mut self) { self.stop(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{cell::Cell, task::Waker};
    use std::rc::Rc;

    struct TestChannel {
        stops: Rc<Cell<usize>>,
        outcome: Poll<Result<(), ()>>,
        fail_start: bool,
    }

    // The mock makes no accesses after start or from other execution contexts.
    unsafe impl DmaChannel for TestChannel {
        type Error = ();

        unsafe fn start(&mut self, buffer: *mut u8, len: usize) -> Result<(), ()> {
            assert!(len > 0);
            unsafe { buffer.write(42) };
            if self.fail_start { Err(()) } else { Ok(()) }
        }

        fn poll_complete(&mut self, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
            self.outcome
        }

        fn stop(&mut self) { self.stops.set(self.stops.get() + 1); }
    }

    #[test]
    fn dma_returns_resources_or_stops_before_drop() {
        let stops = Rc::new(Cell::new(0));
        let channel = |outcome, fail_start| TestChannel { stops: stops.clone(), outcome, fail_start };
        let mut cx = Context::from_waker(Waker::noop());
        for (outcome, fail_start) in [(Ok(()), false), (Err(()), false), (Err(()), true)] {
            let payload = Box::<[u8]>::from([0]);
            let address = payload.as_ptr();
            let before = stops.get();
            let mut operation = channel(Poll::Ready(outcome), fail_start).transfer(payload);
            let Poll::Ready(done) = Pin::new(&mut operation).poll(&mut cx) else { panic!() };
            assert_eq!(done.result, outcome);
            assert_eq!(done.buffer.as_ptr(), address);
            assert_eq!(&*done.buffer, &[42]);
            drop(operation);
            assert_eq!(stops.get(), before + 1);
        }
        let before = stops.get();
        let mut active = channel(Poll::Pending, false).transfer(Box::from([1]));
        assert!(Pin::new(&mut active).poll(&mut cx).is_pending());
        let (recovered, buffer) = active.cancel();
        assert_eq!(stops.get(), before + 1);
        drop(recovered.transfer(buffer)); // Unpolled operation never started DMA.
        assert_eq!(stops.get(), before + 1);
        let mut active = channel(Poll::Pending, false).transfer(Box::from([1]));
        assert!(Pin::new(&mut active).poll(&mut cx).is_pending());
        drop(active);
        assert_eq!(stops.get(), before + 2);
        let mut empty = channel(Poll::Pending, false).transfer(Box::from([]));
        assert!(Pin::new(&mut empty).poll(&mut cx).is_ready());
        assert_eq!(stops.get(), before + 2);
    }
}
