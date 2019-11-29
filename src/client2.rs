
use std::collections::HashMap;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::time::Duration;
use std::sync::{Arc, Mutex};

use futures::{StreamExt, SinkExt};
use futures::channel::mpsc;
use futures_codec::{Framed, JsonCodec};

use async_std::future::timeout;
use async_std::os::unix::net::UnixStream;
use async_std::task;

use tracing::{Level, event, span, instrument};

use dsf_rpc::{RequestKind, ResponseKind,
        Request as RpcRequest, Response as RpcResponse};


use crate::error::Error;

type RequestMap = Arc<Mutex<HashMap<u64, mpsc::Sender<ResponseKind>>>>;

#[derive(Debug, Clone)]
pub struct Client {
    sink: mpsc::Sender<RpcRequest>,
    requests: RequestMap,

    timeout: Duration,
}

impl Client {
    /// Create a new client
    #[instrument]
    pub fn new(addr: &str, timeout: Duration) -> Result<Self, Error> {
        event!(Level::INFO, "Client connecting (address: {})", addr);

        // Connect to stream
        let stream = StdUnixStream::connect(addr)?;
        let stream = UnixStream::from(stream);

        // Build codec and split
        let codec = JsonCodec::<RpcRequest, RpcResponse>::new();
        let framed = Framed::new(stream, codec);
        let (mut unix_sink, mut unix_stream) = framed.split();


        // Create sending task
        let (internal_sink, mut internal_stream) = mpsc::channel::<RpcRequest>(0);
        task::spawn(async move {
            event!(Level::DEBUG, "started client tx listener");
            while let Some(msg) = internal_stream.next().await {
                unix_sink.send(msg).await.unwrap();
            }
            ()
        });

        // Create receiving task
        let requests = Arc::new(Mutex::new(HashMap::new()));
        let reqs = requests.clone();
        task::spawn(async move {
            event!(Level::DEBUG, "started client rx listener");
            while let Some(Ok(resp)) = unix_stream.next().await {
                Self::handle(&reqs, resp).await.unwrap();
            }
            ()
        });

        Ok(Client{ sink: internal_sink, requests, timeout })
    }

    /// Issue a request using a client instance
    // TODO: #[instrument] when futures 0.3 support is viable
    pub async fn request(&mut self, rk: RequestKind) -> Result<ResponseKind, Error> {
        let (tx, mut rx) = mpsc::channel(0);
        let req = RpcRequest::new(rk);
        let id = req.req_id();
        
        span!(Level::DEBUG, "Issuing request", ?req);

        // Add to tracking
        self.requests.lock().unwrap().insert(id, tx);

        // Send message
        self.sink.send(req).await.unwrap();

        // Await and return response
        let res = timeout(self.timeout, rx.next() ).await;

        // TODO: Handle timeout errors
        let res = match res {
            Ok(Some(v)) => Ok(v),
            // TODO: this seems like it should be a yeild / retry point..?
            Ok(None) => {
                event!(Level::ERROR, "No response received");
                Err(Error::None(()))
            },
            Err(e) => {
                event!(Level::ERROR, "Response error: {:?}", e);
                Err(Error::Timeout)
            },
        };

        // Remove request on failure
        if let Err(_e) = &res {
            self.requests.lock().unwrap().remove(&id);
        }

        res
    }

    async fn handle(requests: &RequestMap, resp: RpcResponse) -> Result<(), Error> {
        // Find matching sender
        let id = resp.req_id();
        let mut a = match requests.lock().unwrap().get_mut(&id) {
            Some(a) => a.clone(),
            None => {
                event!(Level::ERROR, "Unix RX with no matching request ID");
                return Err(Error::Unknown)
            },
        };

        // Forward response
        match a.send(resp.kind()).await {
            Ok(_) => (),
            Err(e) => {
                event!(Level::ERROR, "client send error: {:?}", e);
            }
        };

        Ok(())
    }
}
