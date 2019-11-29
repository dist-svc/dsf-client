
use std::io;
use std::collections::HashMap;
use std::time::Duration;
use std::os::unix::net::UnixStream as StdUnixStream;

use futures::{future, Stream, Sink, Future};
use futures::channel::mpsc;

use tokio::prelude::FutureExt;
use tokio::reactor::Handle;

use actix::prelude::*;

use tokio::codec::Framed;

#[cfg(unix)]
use tokio_uds::{UnixStream};

#[cfg(windows)]
use tokio_uds_windows::{UnixListener, UnixStream};

use daemon_engine::JsonCodec;

//use dsf_core::types::{Error};

use dsf_rpc as rpc;

use crate::error::Error;

/// ClientActor connects to a DSF daemon using a unix socket
pub struct ClientActor {
    //sink: SplitSink<Framed<UnixStream, JsonCodec::<rpc::Request, rpc::Response>>>,
    sink: mpsc::Sender<rpc::Request>,
    requests: HashMap<u64, mpsc::Sender<rpc::ResponseKind>>,
    timeout: Duration,
}

impl ClientActor {
    pub fn new(path: &str) -> Result<Addr<Self>, io::Error> {
        debug!("Connecting to socket: {}", path);

        // Connect to socket
        let socket = StdUnixStream::connect(path)?;
        let socket = UnixStream::from_std(socket, &Handle::default()).unwrap();

        // Configure codec
        let codec = JsonCodec::<rpc::Request, rpc::Response>::new();
        let (unix_sink, unix_stream) = Framed::new(socket, codec).split();

        let (internal_sink, internal_stream) = mpsc::channel::<rpc::Request>(0);

        debug!("Connected");

        Ok(Self::create(move |ctx| {
            // Map internal stream to unix sink to allow sending futures
            tokio::spawn( unix_sink.sink_map_err(|_| ()).send_all(internal_stream).map(|_| () ).map_err(|_| ()) );

            // Map unix stream to internal handler
            ctx.add_stream(unix_stream.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{:?}", e))));

            // Return sink object
            Self{ sink: internal_sink, requests: HashMap::new(), timeout: Duration::from_secs(10) }
        }))
    }
}

impl Actor for ClientActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        trace!("Started ClientActor");

        //self.subscribe_sync::<ArbiterBroker, RpcResponse>(ctx);
    }

    fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
        trace!("Stopped ClientActor");

        Running::Stop
    }
}

/// Stream handler for incoming RPC responses
impl StreamHandler<rpc::Response, io::Error> for ClientActor {
    fn handle(&mut self, resp: rpc::Response, ctx: &mut Context<Self>) {
        let a = ctx.address();
        a.do_send(RpcResponse(resp));
    }
}

/// Wrapper for incoming RPC response messages to support async operations
struct RpcResponse (rpc::Response);

impl Message for RpcResponse {
    type Result = Result<(), Error>;
}

/// Actual RpcResponse handler
/// This forwards the message to a pending handle or exits if done
impl Handler<RpcResponse> for ClientActor {
    type Result = ResponseFuture<(), Error>;

    fn handle(&mut self, resp: RpcResponse, _ctx: &mut Context<Self>) -> Self::Result {
        let resp = resp.0;

        debug!("Unix socket RX: {:?}", resp);
        let req_id = resp.req_id();

        // Locate pending actors
        let a = match self.requests.get(&req_id) {
            Some(a) => a.clone(),
            None => {
                 error!("Unix RX with no matching request ID");
                 return Box::new(future::err(Error::Unknown))
            },
        };

        // Send request to pending actors
        Box::new( a.send(resp.kind()).map(|_| () ).map_err(|_| Error::Unknown ) )
    }
}

#[derive(Debug, PartialEq, Clone, Message)]
pub struct RequestDone(pub u64);

impl Handler<RequestDone> for ClientActor {
    type Result = ();

    fn handle(&mut self, req: RequestDone, _ctx: &mut Context<Self>) -> Self::Result {
        debug!("Received request complete");
        self.requests.remove(&req.0);
    }
}

/// RpcRequest wraps rpc::Request with Addr<RequestActor> return
#[derive(Debug)]
pub struct SingleRequest(pub rpc::Request);

impl Message for SingleRequest { 
    type Result = Result<rpc::ResponseKind, Error>; 
}

/// Handler to issue RPC requests
impl Handler<SingleRequest> for ClientActor {
    type Result = ResponseFuture<rpc::ResponseKind, Error>;

    fn handle(&mut self, req: SingleRequest, ctx: &mut Context<Self>) -> Self::Result {
        let (tx, rx) = mpsc::channel(0);

        debug!("Issuing single request: {:?}", req);

        let req_id = req.0.req_id();
        let a = ctx.address();

        // Add to tracking
        self.requests.insert(req_id, tx);

        // Build send future
        let send = self.sink.clone().send(req.0)
            .map_err(|_e| Error::Socket);
    
        // Build receive future
        let receive = rx.into_future().map_err(|_e| Error::Socket)
            .timeout(self.timeout).map_err(|e| e.into() ).map(|(r, _f)| r.unwrap() );

        // Compose and return future
        Box::new(
            send.and_then(|_| receive )
            // Drop pending request and return
            .then(move |r| {
                a.do_send(RequestDone(req_id));
                r
            })
        )
    }
}


/// StreamRequest wraps rpc::Request and allows responses to be streamed
#[derive(Debug)]
pub struct StreamRequest(pub rpc::Request);

impl Message for StreamRequest {
    type Result = Result<(rpc::ResponseKind, mpsc::Receiver<rpc::ResponseKind>), Error>;
}

/// Handler to issue RPC requests
impl Handler<StreamRequest> for ClientActor {
    type Result = ResponseFuture<(rpc::ResponseKind, mpsc::Receiver<rpc::ResponseKind>), Error>;

    fn handle(&mut self, req: StreamRequest, ctx: &mut Context<Self>) -> Self::Result {
        let (tx, rx) = mpsc::channel(0);
        let a = ctx.address();
        let req_id = req.0.req_id();

        debug!("Issuing stream request: {:?}", req);

        // Add to tracking
        self.requests.insert(req_id, tx);

        // Build send future
        let send = self.sink.clone().send(req.0)
            .map_err(|_e| Error::Socket);
    
        // Build receive future
        let receive = rx.into_future().map_err(|_e| Error::Socket)
            .timeout(self.timeout).map_err(|e| e.into() ).map(|(r, rx)| (r.unwrap(), rx) );

        Box::new(
            send.and_then(|_| receive )
            // Drop the pending request ID on error
            .map_err(move |e| {
                a.do_send(RequestDone(req_id));
                e
            })
        )
    }
}
