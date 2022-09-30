use std::ops::{Deref, DerefMut};

use bytes::buf::UninitSlice;
use bytes::BufMut;

pub(crate) struct IoBuf {
  buf: Vec<u8>,
  head: usize,
  tail: usize,
}

impl IoBuf {
  pub fn with_capacity(capacity: usize) -> Self {
    Self {
      buf: vec![0; capacity],
      head: 0,
      tail: 0,
    }
  }

  pub fn capacity(&self) -> usize {
    self.buf.len()
  }

  /// The number of extra bytes that available to write to
  pub fn extra(&self) -> usize {
    self.capacity() - self.tail
  }

  pub fn as_slice(&self) -> &[u8] {
    &self.buf[self.head..self.tail]
  }

  pub fn as_slice_mut(&mut self) -> &mut [u8] {
    &mut self.buf[self.head..self.tail]
  }
}

impl IoBuf {
  pub fn consume(&mut self, len: usize) {
    assert!(len <= self.len(), "{} >= {}", len, self.len());

    self.head += len;

    // Shift buffer back if we have too much empty space
    if self.head > self.len() {
      let range = self.head..self.tail;
      self.buf.copy_within(range, 0);
      self.tail -= self.head;
      self.head -= self.head;
    }
  }

  /// Mark the next len bytes as filled
  pub fn advance(&mut self, len: usize) {
    assert!(len <= self.extra());

    self.tail += len;
  }

  pub fn remainder(&mut self, minimum: usize) -> &mut [u8] {
    if self.extra() < minimum {
      let newcap = (self.buf.len() * 2).max(self.buf.len() + minimum);
      self.buf.resize(newcap, 0);
    }

    &mut self.buf[self.tail..]
  }
}

impl Deref for IoBuf {
  type Target = [u8];

  fn deref(&self) -> &Self::Target {
    self.as_slice()
  }
}

impl DerefMut for IoBuf {
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.as_slice_mut()
  }
}

unsafe impl BufMut for IoBuf {
  fn remaining_mut(&self) -> usize {
    self.extra()
  }

  unsafe fn advance_mut(&mut self, cnt: usize) {
    self.advance(cnt)
  }

  fn chunk_mut(&mut self) -> &mut UninitSlice {
    let remainder = self.remainder(1024);
    unsafe { UninitSlice::from_raw_parts_mut(remainder.as_mut_ptr(), remainder.len()) }
  }
}
