
use std::collections::HashMap;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::time::Duration;
use std::sync::{Arc, Mutex};

use futures::{StreamExt, SinkExt};
use futures::channel::mpsc;
use futures_codec::{Framed, JsonCodec};

use async_std::future::timeout;
use async_std::os::unix::net::UnixStream;
use async_std::task::{self, JoinHandle};

use async_trait::async_trait;

use tracing::{Level, span};

use dsf_rpc::*;
use dsf_rpc::{Request as RpcRequest, Response as RpcResponse};

use dsf_core::api::*;

use crate::error::Error;

type RequestMap = Arc<Mutex<HashMap<u64, mpsc::Sender<ResponseKind>>>>;

#[derive(Debug)]
pub struct Client {
    addr: String,
    sink: mpsc::Sender<RpcRequest>,
    requests: RequestMap,

    timeout: Duration,

    tx_handle: JoinHandle<()>,
    rx_handle: JoinHandle<()>,
}

impl Client {
    /// Create a new client
    pub fn new(addr: &str, timeout: Duration) -> Result<Self, Error> {
        let span = span!(Level::DEBUG, "client", "{}", addr);
        let _enter = span.enter();

        info!("Client connecting (address: {})", addr);

        // Connect to stream
        let stream = StdUnixStream::connect(addr)?;
        let stream = UnixStream::from(stream);

        // Build codec and split
        let codec = JsonCodec::<RpcRequest, RpcResponse>::new();
        let framed = Framed::new(stream, codec);
        let (mut unix_sink, mut unix_stream) = framed.split();

        // Create sending task
        let (internal_sink, mut internal_stream) = mpsc::channel::<RpcRequest>(0);
        let tx_handle = task::spawn(async move {
            trace!("started client tx listener");
            while let Some(msg) = internal_stream.next().await {
                unix_sink.send(msg).await.unwrap();
            }
            ()
        });

        // Create receiving task
        let requests = Arc::new(Mutex::new(HashMap::new()));
        let reqs = requests.clone();
        let rx_handle = task::spawn(async move {
            trace!("started client rx listener");
            while let Some(Ok(resp)) = unix_stream.next().await {
                Self::handle(&reqs, resp).await.unwrap();
            }
            ()
        });

        Ok(Client{ sink: internal_sink, addr: addr.to_owned(), requests, timeout, rx_handle, tx_handle })
    }

    /// Issue a request using a client instance
    // TODO: #[instrument] when futures 0.3 support is viable
    pub async fn request(&mut self, rk: RequestKind) -> Result<ResponseKind, Error> {
        let span = span!(Level::DEBUG, "client", "{}", self.addr);
        let _enter = span.enter();

        debug!("Issuing request: {:?}", rk);

        let resp = self.do_request(rk).await.map(|(v, _)| v )?;

        debug!("Received response: {:?}", resp);

        Ok(resp)
    }
    
    // TODO: #[instrument]
    async fn do_request(&mut self, rk: RequestKind) -> Result<(ResponseKind, mpsc::Receiver<ResponseKind>), Error> {

        let (tx, mut rx) = mpsc::channel(0);
        let req = RpcRequest::new(rk);
        let id = req.req_id();

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
                error!("No response received");
                Err(Error::None(()))
            },
            Err(e) => {
                error!("Response error: {:?}", e);
                Err(Error::Timeout)
            },
        };

        // Remove request on failure
        if let Err(_e) = &res {
            self.requests.lock().unwrap().remove(&id);
        }

        res.map(|v| (v, rx) )
    }

    // Internal function to handle received messages
    async fn handle(requests: &RequestMap, resp: RpcResponse) -> Result<(), Error> {
        // Find matching sender
        let id = resp.req_id();
        let mut a = match requests.lock().unwrap().get_mut(&id) {
            Some(a) => a.clone(),
            None => {
                error!("Unix RX with no matching request ID");
                return Err(Error::Unknown)
            },
        };

        // Forward response
        match a.send(resp.kind()).await {
            Ok(_) => (),
            Err(e) => {
                error!("client send error: {:?}", e);
            }
        };

        Ok(())
    }

    /// Fetch daemon status information
    pub async fn status(&mut self) -> Result<dsf_rpc::StatusInfo, Error> {
        let req = RequestKind::Status;
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Status(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }

    /// Connect to another DSF instance
    pub async fn connect(&mut self, options: dsf_rpc::peer::ConnectOptions) -> Result<dsf_rpc::peer::ConnectInfo, Error> {
        let req = RequestKind::Peer(dsf_rpc::peer::PeerCommands::Connect(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Connected(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }

    /// Search for a peer using the database
    pub async fn find(&mut self, options: dsf_rpc::peer::SearchOptions) -> Result<dsf_rpc::peer::PeerInfo, Error> {
        let req = RequestKind::Peer(dsf_rpc::peer::PeerCommands::Search(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Peers(info) => Ok(info[0].1.clone()),
            _ => Err(Error::UnrecognizedResult),
        }
    }

    /// Search for a peer using the database
    pub async fn list(&mut self, options: dsf_rpc::service::ListOptions) -> Result<Vec<dsf_rpc::service::ServiceInfo>, Error> {
        let req = RequestKind::Service(dsf_rpc::service::ServiceCommands::List(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Services(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}

#[async_trait]
impl Create for Client {
    type Options = dsf_rpc::service::CreateOptions;
    type Error = Error;

    /// Create a new service with the provided options
    /// This MUST be stored locally for reuse
    async fn create(&mut self, options: dsf_rpc::service::CreateOptions) -> Result<ServiceHandle, Self::Error> {
        let req = RequestKind::Service(dsf_rpc::service::ServiceCommands::Create(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Created(info) => Ok(ServiceHandle::new(info.id.clone())),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}

#[async_trait]
impl Register for Client {
    type Options = dsf_rpc::service::RegisterOptions;
    type Info = dsf_rpc::service::RegisterInfo;
    type Error = Error;

    /// Register a service instance in the distributed database
    async fn register(&mut self, options: Self::Options) -> Result<Self::Info, Self::Error> {
        let req = RequestKind::Service(dsf_rpc::service::ServiceCommands::Register(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Registered(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}

#[async_trait]
impl Locate for Client {
    type Options = dsf_rpc::service::LocateOptions;
    type Info = dsf_rpc::service::LocateInfo;
    type Error = Error;

    /// Locate a service instance in the distributed database
    /// This returns a future that will resolve to the desired service or an error
    async fn locate(&mut self, options: Self::Options) -> Result<Self::Info, Self::Error> {
        let req = RequestKind::Service(dsf_rpc::service::ServiceCommands::Search(options));

        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Located(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}

#[async_trait]
impl Publish for Client {
    type Options = dsf_rpc::data::PublishOptions;
    type Info = dsf_rpc::data::PublishInfo;
    type Error = Error;

    /// Publish data using an existing service
    async fn publish(&mut self, options: Self::Options) -> Result<Self::Info, Self::Error> {

        let req = RequestKind::Data(DataCommands::Publish(options));
        
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Published(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}


pub struct SubscribeOptions{

}

impl Default for SubscribeOptions {
    fn default() -> Self {
        Self {}
    }
}

#[async_trait]
impl Subscribe for Client {
    type Options = dsf_rpc::service::SubscribeOptions;
    type Streamable = mpsc::Receiver<ResponseKind>;
    type Error = Error;

    /// Subscribe to data from a given service
    async fn subscribe(&mut self, options: Self::Options) -> Result<Self::Streamable, Self::Error> {
        
        let req = RequestKind::Stream(dsf_rpc::StreamOptions{service: options.service});
        
        let (resp, rx) = self.do_request(req).await?;

        match resp {
            ResponseKind::Subscribed(_info) => Ok(rx),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}
