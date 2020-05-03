use std::collections::HashMap;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc;
use futures::prelude::*;
use futures::{SinkExt, StreamExt};

use futures_codec::{Framed, JsonCodec};

use async_std::future::timeout;
use async_std::os::unix::net::UnixStream;
use async_std::task::{self, JoinHandle};

//use async_trait::async_trait;

use tracing::{span, Level};

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
        });

        // Create receiving task
        let requests = Arc::new(Mutex::new(HashMap::new()));
        let reqs = requests.clone();
        let rx_handle = task::spawn(async move {
            trace!("started client rx listener");
            while let Some(Ok(resp)) = unix_stream.next().await {
                Self::handle(&reqs, resp).await.unwrap();
            }
        });

        Ok(Client {
            sink: internal_sink,
            addr: addr.to_owned(),
            requests,
            timeout,
            rx_handle,
            tx_handle,
        })
    }

    /// Issue a request using a client instance
    // TODO: #[instrument] when futures 0.3 support is viable
    pub async fn request(&mut self, rk: RequestKind) -> Result<ResponseKind, Error> {
        let span = span!(Level::DEBUG, "client", "{}", self.addr);
        let _enter = span.enter();

        debug!("Issuing request: {:?}", rk);

        let resp = self.do_request(rk).await.map(|(v, _)| v)?;

        debug!("Received response: {:?}", resp);

        Ok(resp)
    }

    // TODO: #[instrument]
    async fn do_request(
        &mut self,
        rk: RequestKind,
    ) -> Result<(ResponseKind, mpsc::Receiver<ResponseKind>), Error> {
        let (tx, mut rx) = mpsc::channel(0);
        let req = RpcRequest::new(rk);
        let id = req.req_id();

        // Add to tracking
        trace!("request add lock");
        self.requests.lock().unwrap().insert(id, tx);

        // Send message
        self.sink.send(req).await.unwrap();

        // Await and return response
        let res = timeout(self.timeout, rx.next()).await;

        // TODO: Handle timeout errors
        let res = match res {
            Ok(Some(v)) => Ok(v),
            // TODO: this seems like it should be a yeild / retry point..?
            Ok(None) => {
                error!("No response received");
                Err(Error::None(()))
            }
            Err(e) => {
                error!("Response error: {:?}", e);
                Err(Error::Timeout)
            }
        };

        // Remove request on failure
        if let Err(_e) = &res {
            trace!("request failure lock");
            self.requests.lock().unwrap().remove(&id);
        }

        res.map(|v| (v, rx))
    }

    // Internal function to handle received messages
    async fn handle(requests: &RequestMap, resp: RpcResponse) -> Result<(), Error> {
        // Find matching sender
        let id = resp.req_id();
        trace!("receive request lock");
        let mut a = match requests.lock().unwrap().get_mut(&id) {
            Some(a) => a.clone(),
            None => {
                error!("Unix RX with no matching request ID");
                return Err(Error::Unknown);
            }
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
    pub async fn status(&mut self) -> Result<StatusInfo, Error> {
        let req = RequestKind::Status;
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Status(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }

    /// Connect to another DSF instance
    pub async fn connect(
        &mut self,
        options: peer::ConnectOptions,
    ) -> Result<peer::ConnectInfo, Error> {
        let req = RequestKind::Peer(peer::PeerCommands::Connect(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Connected(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }

    /// Search for a peer using the database
    pub async fn find(&mut self, options: peer::SearchOptions) -> Result<peer::PeerInfo, Error> {
        let req = RequestKind::Peer(peer::PeerCommands::Search(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Peers(info) => Ok(info[0].1.clone()),
            _ => Err(Error::UnrecognizedResult),
        }
    }

    /// Search for a peer using the database
    pub async fn list(
        &mut self,
        options: service::ListOptions,
    ) -> Result<Vec<service::ServiceInfo>, Error> {
        let req = RequestKind::Service(service::ServiceCommands::List(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Services(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }

    pub async fn info(
        &mut self,
        options: service::InfoOptions,
    ) -> Result<(ServiceHandle, ServiceInfo), Error> {
        let req = RequestKind::Service(service::ServiceCommands::Info(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Service(info) => Ok((ServiceHandle::new(info.id.clone()), info)),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}

//#[async_trait]
impl Client {
    //type Options = dsf_rpc::service::CreateOptions;
    //type Error = Error;

    /// Create a new service with the provided options
    /// This MUST be stored locally for reuse
    pub async fn create(
        &mut self,
        options: service::CreateOptions,
    ) -> Result<ServiceHandle, Error> {
        let req = RequestKind::Service(service::ServiceCommands::Create(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Service(info) => Ok(ServiceHandle::new(info.id.clone())),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}

//#[async_trait]
impl Client {
    //type Error = ();

    /// Register a service instance in the distributed database
    pub async fn register(
        &mut self,
        options: RegisterOptions,
    ) -> Result<dsf_rpc::service::RegisterInfo, Error> {
        let req = RequestKind::Service(dsf_rpc::service::ServiceCommands::Register(options));
        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Registered(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}

//#[async_trait]
impl Client {
    //type Error = Error;

    /// Locate a service instance in the distributed database
    /// This returns a future that will resolve to the desired service or an error
    pub async fn locate(
        &mut self,
        options: LocateOptions,
    ) -> Result<(ServiceHandle, LocateInfo), Error> {
        let id = options.id.clone();
        let req = RequestKind::Service(dsf_rpc::service::ServiceCommands::Locate(options));

        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Located(info) => {
                let handle = ServiceHandle { id: id.clone() };
                Ok((handle, info))
            }
            _ => Err(Error::UnrecognizedResult),
        }
    }
}

//#[async_trait]
impl Client {
    //type Error = Error;

    /// Publish data using an existing service
    pub async fn publish(&mut self, options: PublishOptions) -> Result<PublishInfo, Error> {
        let req = RequestKind::Data(DataCommands::Publish(options));

        let resp = self.request(req).await?;

        match resp {
            ResponseKind::Published(info) => Ok(info),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}

//#[async_trait]
impl Client {
    //type Options = SubscribeOptions;
    //type Data = ResponseKind;
    //type Error = Error;

    /// Subscribe to data from a given service
    pub async fn subscribe(
        &mut self,
        options: SubscribeOptions,
    ) -> Result<impl Stream<Item = ResponseKind>, Error> {
        let req = RequestKind::Service(ServiceCommands::Subscribe(options));

        let (resp, rx) = self.do_request(req).await?;

        //let rx: Box<dyn Stream<Item=ResponseKind>> = Box::new(rx);

        match resp {
            ResponseKind::Subscribed(_info) => Ok(rx),
            _ => Err(Error::UnrecognizedResult),
        }
    }
}
