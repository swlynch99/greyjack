use crate::Compose;

pub type NoResponse = std::convert::Infallible;

pub enum ParseError<E> {
  Incomplete,
  Fatal,
  Error { response: E, consumed: usize },
}

pub struct ParseOk<T> {
  value: T,
  consumed: usize,
}

impl<T> ParseOk<T> {
  pub fn new(value: T, consumed: usize) -> Self {
    Self { value, consumed }
  }

  pub fn consumed(&self) -> usize {
    self.consumed
  }

  pub fn inner(&self) -> &T {
    &self.value
  }

  pub fn into_inner(self) -> T {
    self.value
  }
}

pub trait Parser: Send {
  type Request<'b>: Send;
  type Error<'b>: Compose + Send;

  fn parse<'b>(
    &self,
    bytes: &'b [u8],
  ) -> Result<ParseOk<Self::Request<'b>>, ParseError<Self::Error<'b>>>;
}

impl Compose for NoResponse {
  fn compose<B: bytes::BufMut>(&self, _: &mut B) {
    match *self {}
  }

  fn should_close(&self) -> bool {
    match *self {}
  }
}
