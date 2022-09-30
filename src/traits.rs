use bytes::BufMut;

pub trait Compose {
  fn compose<B: BufMut>(&self, bytes: &mut B);

  fn should_close(&self) -> bool {
    false
  }
}

#[async_trait]
pub trait Handler: Send {
  type Request<'b>: Send;
  type Response<'b>: Compose + Send;

  async fn execute<'b>(&mut self, request: Self::Request<'b>) -> Self::Response<'b>;
}

pub trait HandlerBuilder: Send + Sync {
  type Handler: Handler;

  fn build(&self) -> Self::Handler;
}
