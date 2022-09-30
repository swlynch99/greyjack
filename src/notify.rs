use std::cell::UnsafeCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::task::{Context, Poll, Waker};

use futures::future::FusedFuture;

struct NotifyData {
  notified: AtomicBool,
  wakers: RwLock<Vec<UnsafeCell<Waker>>>,
}

impl NotifyData {
  fn with_capacity(capacity: usize) -> Self {
    Self {
      notified: AtomicBool::new(false),
      wakers: RwLock::new(Vec::with_capacity(capacity)),
    }
  }
}

unsafe impl Send for NotifyData {}
unsafe impl Sync for NotifyData {}

/// This is a one-shot async notifying future.
pub(crate) struct Notify(NotifyData);

impl Notify {
  pub fn new() -> Self {
    Self::with_capacity(8)
  }

  pub fn with_capacity(capacity: usize) -> Self {
    Self(NotifyData::with_capacity(capacity))
  }

  pub fn watch(&self) -> Watcher {
    Watcher::new(&self.0)
  }

  pub fn notify(&self) {
    // Don't re-notify
    if self.0.notified.load(Ordering::Acquire) {
      return;
    }

    let mut lock = self
      .0
      .wakers
      .write()
      .expect("notify data lock was poisoned");

    self.0.notified.store(true, Ordering::Release);

    for waker in lock.drain(..) {
      waker.into_inner().wake();
    }
  }
}

pub(crate) struct Watcher<'n> {
  data: &'n NotifyData,
  index: usize,
}

impl<'n> Watcher<'n> {
  fn new(data: &'n NotifyData) -> Self {
    Self {
      data,
      index: usize::MAX,
    }
  }
}

impl<'n> Future for Watcher<'n> {
  type Output = ();

  fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    if self.data.notified.load(Ordering::Acquire) {
      return Poll::Ready(());
    }

    let mut waker = cx.waker().clone();
    match self.index {
      usize::MAX => {
        let mut lock = self
          .data
          .wakers
          .write()
          .expect("notify data lock was poisoned");

        // Avoid race conditions
        if self.data.notified.load(Ordering::Acquire) {
          return Poll::Ready(());
        }

        let index = lock.len();
        lock.push(UnsafeCell::new(waker));
        self.index = index;
      }
      index => {
        let lock = self
          .data
          .wakers
          .read()
          .expect("notify data lock was poisoned");

        // Avoid race conditions
        if self.data.notified.load(Ordering::Acquire) {
          return Poll::Ready(());
        }

        // SAFETY: We are in a read lock and only modifying our own index.
        //         Other threads in a read lock will only modify their own
        //         indices which will be different (since they could not be
        //         polling this watcher instance).
        unsafe { std::mem::swap(&mut *lock[index].get(), &mut waker) };
      }
    }

    Poll::Pending
  }
}

impl FusedFuture for Watcher<'_> {
  fn is_terminated(&self) -> bool {
    self.data.notified.load(Ordering::Acquire)
  }
}
