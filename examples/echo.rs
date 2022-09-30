use greyjack::{
  Compose, Handler, HandlerBuilder, NoResponse, ParseError, ParseOk, Parser, Server, Worker,
};

struct Data<'b>(&'b [u8]);

impl Compose for Data<'_> {
  fn compose<B: bytes::BufMut>(&self, bytes: &mut B) {
    bytes.put(self.0)
  }
}

#[derive(Clone)]
struct DataParser;

impl Parser for DataParser {
  type Error<'b> = NoResponse;
  type Request<'b> = Data<'b>;

  fn parse<'b>(
    &self,
    bytes: &'b [u8],
  ) -> Result<ParseOk<Self::Request<'b>>, ParseError<Self::Error<'b>>> {
    Ok(ParseOk::new(Data(bytes), bytes.len()))
  }
}

#[derive(Clone)]
struct Echo;

#[async_trait::async_trait]
impl Handler for Echo {
  type Request<'b> = Data<'b>;
  type Response<'b> = Data<'b>;

  async fn execute<'b>(&mut self, request: Self::Request<'b>) -> Self::Response<'b> {
    request
  }
}

impl HandlerBuilder for Echo {
  type Handler = Self;

  fn build(&self) -> Self::Handler {
    Echo
  }
}

fn main() {
  Server::new(8000)
    .workers(1, Worker::new(Echo, DataParser))
    .run();
}
