//!

#[macro_use]
extern crate async_trait;

mod iobuf;
mod notify;
mod parse;
mod traits;

use std::thread::{Scope, ScopedJoinHandle};
use std::time::Duration;

use async_channel::Receiver;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::iobuf::IoBuf;
use crate::notify::{Notify, Watcher};
pub use crate::parse::{NoResponse, ParseError, ParseOk, Parser};
pub use crate::traits::{Compose, Handler, HandlerBuilder};

const DEFAULT_RDBUF_CAPACITY: usize = 16384;
const DEFAULT_WRBUF_CAPACITY: usize = 16384;
const DEFAULT_READ_SIZE: usize = 2048;

#[derive(Clone)]
pub struct Worker<B, P> {
  builder: B,
  parser: P,
}

impl<B, P> Worker<B, P>
where
  B: HandlerBuilder + Send + Sync + 'static,
  P: for<'b> Parser<Request<'b> = <<B as HandlerBuilder>::Handler as Handler>::Request<'b>>,
  P: Clone + Sync + 'static,
{
  pub fn new(builder: B, parser: P) -> Self {
    Self { builder, parser }
  }

  fn launch<'s, 'e: 's>(
    &'e self,
    scope: &'s Scope<'s, 'e>,
    watcher: Watcher<'e>,
    rx: Receiver<TcpStream>,
  ) -> ScopedJoinHandle<'s, ()> {
    scope.spawn(move || {
      tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("unable to create worker runtime")
        .block_on(self.run(watcher, rx));
    })
  }

  async fn run<'e>(&'e self, mut watcher: Watcher<'e>, rx: Receiver<TcpStream>) {
    loop {
      let conn = tokio::select! {
        () = &mut watcher => return,
        result = rx.recv() => match result {
          Ok(conn) => conn,
          Err(_) => return
        }
      };

      println!("got new connection!");

      let handler = self.builder.build();
      let parser = self.parser.clone();

      tokio::spawn(async move {
        let parser = parser;
        let mut handler = handler;
        let mut conn = conn;
        let mut rdbuf = IoBuf::with_capacity(DEFAULT_RDBUF_CAPACITY);
        let mut wrbuf = Vec::with_capacity(DEFAULT_WRBUF_CAPACITY);
        let mut unparsed_extra = false;
        let mut exit = false;

        while !exit {
          if !unparsed_extra || rdbuf.is_empty() {
            let extra = rdbuf.remainder(DEFAULT_READ_SIZE);
            match conn.read(extra).await {
              Ok(count) => rdbuf.advance(count),
              Err(_) => return,
            };

            unparsed_extra = true;
          }

          let consumed = match parser.parse(&rdbuf) {
            Ok(result) => {
              let consumed = result.consumed();
              let request = result.into_inner();
              let response = handler.execute(request).await;
              response.compose(&mut wrbuf);
              exit = response.should_close();

              consumed
            }
            Err(e) => match e {
              ParseError::Fatal => {
                return;
              }
              ParseError::Incomplete => {
                unparsed_extra = false;
                0
              }
              ParseError::Error { consumed, response } => {
                response.compose(&mut wrbuf);
                exit = response.should_close();
                consumed
              }
            },
          };

          rdbuf.consume(consumed);
          if let Err(_) = conn.write_all(&wrbuf).await {
            return;
          }

          wrbuf.clear();
        }
      });
    }
  }
}

pub struct Server<WorkerBuilder, WorkerParser> {
  workers: Vec<Worker<WorkerBuilder, WorkerParser>>,
  port: u16,
}

impl<B, P> Server<B, P>
where
  B: HandlerBuilder + Send + Sync + 'static,
  P: for<'b> Parser<Request<'b> = <<B as HandlerBuilder>::Handler as Handler>::Request<'b>>,
  P: Clone + Sync + 'static,
{
  pub fn new(port: u16) -> Self {
    Self {
      workers: Vec::new(),
      port,
    }
  }

  pub fn workers(&mut self, num: usize, config: Worker<B, P>) -> &mut Self
  where
    Worker<B, P>: Clone,
  {
    self.workers = vec![config; num];
    self
  }

  pub fn run(&mut self) {
    let notify = Notify::new();

    std::thread::scope(|scope| {
      let (tx, rx) = async_channel::bounded(1024);
      for worker in &self.workers {
        let _ = worker.launch(scope, notify.watch(), rx.clone());
      }

      std::thread::sleep(Duration::from_secs(1));

      tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("unable to create worker runtime")
        .block_on(async {
          let _guard = drop_guard::guard((), |()| notify.notify());

          let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await
            .expect("Failed to bind to TCP port");

          while let Ok((conn, _)) = listener.accept().await {
            let _ = tx.send(conn).await;
          }
        })
    });
  }
}
