//!

#[macro_use]
extern crate async_trait;
#[macro_use]
extern crate log;

mod iobuf;
mod notify;
mod parse;
mod traits;

use std::future::Future;
use std::os::unix::prelude::AsRawFd;
use std::thread::Scope;
use std::time::Duration;

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
  ) -> tokio::runtime::Handle {
    let rt = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .expect("unable to create worker runtime");

    let handle = rt.handle().clone();

    scope.spawn(move || {
      rt.block_on(async { watcher.await });
    });

    handle
  }

  fn drive(&self, mut conn: TcpStream) -> impl Future<Output = ()> + Send {
    let mut handler = self.builder.build();
    let parser = self.parser.clone();

    async move {
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
      let handles = self
        .workers
        .iter()
        .map(|worker| (worker, worker.launch(scope, notify.watch())))
        .collect::<Vec<_>>();

      std::thread::sleep(Duration::from_secs(1));

      tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("unable to create worker runtime")
        .block_on(async {
          let _guard = drop_guard::guard((), |()| notify.notify());
          let mut index = 0;

          let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await
            .expect("Failed to bind to TCP port");

          while let Ok((conn, _)) = listener.accept().await {
            info!("Accepted new connection with fd {}", conn.as_raw_fd());
            let (worker, handle) = &handles[index];
            let _ = handle.spawn(worker.drive(conn));
            index = (index + 1) % handles.len();
          }
        })
    });
  }
}
