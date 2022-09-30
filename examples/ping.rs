use bytes::BufMut;
use greyjack::{Compose, Handler, HandlerBuilder, ParseError, ParseOk, Parser, Server, Worker};

const PING: &[u8] = b"PING\r\n";
const PONG: &[u8] = b"PONG\r\n";
const BAD: &[u8] = b"BAD\r\n";

struct Ping;
struct Pong;

impl Compose for Pong {
  fn compose<B: BufMut>(&self, bytes: &mut B) {
    bytes.put(PONG)
  }
}

struct BadRequest;

impl Compose for BadRequest {
  fn compose<B: BufMut>(&self, bytes: &mut B) {
    bytes.put(BAD);
  }

  fn should_close(&self) -> bool {
    true
  }
}

#[derive(Clone)]
struct PingParser;

impl Parser for PingParser {
  type Error<'b> = BadRequest;
  type Request<'b> = Ping;

  fn parse<'b>(
    &self,
    bytes: &'b [u8],
  ) -> Result<ParseOk<Self::Request<'b>>, ParseError<Self::Error<'b>>> {
    if !bytes.starts_with(PING) {
      return Err(ParseError::Error {
        response: BadRequest,
        consumed: 0,
      });
    }

    Ok(ParseOk::new(Ping, PING.len()))
  }
}

#[derive(Clone)]
struct PingServer;

#[async_trait::async_trait]
impl Handler for PingServer {
  type Request<'b> = Ping;
  type Response<'b> = Pong;

  async fn execute<'b>(&mut self, _: Self::Request<'b>) -> Self::Response<'b> {
    Pong
  }
}

impl HandlerBuilder for PingServer {
  type Handler = Self;

  fn build(&self) -> Self::Handler {
    Self
  }
}

fn main() {
  console_subscriber::init();
  env_logger::init_from_env("GREYJACK_LOG");

  Server::new(12321)
    .workers(1, Worker::new(PingServer, PingParser))
    .run();
}
