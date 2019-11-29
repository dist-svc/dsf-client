

use std::time::Duration;

use futures::prelude::*;
use actix::{Addr};


//use dsf_core::types::Error as DsfError;
use dsf_core::types::{Id, DataKind};
use dsf_core::api::*;
use dsf_rpc::*;

use crate::error::Error;
use crate::actor::{ClientActor, SingleRequest, StreamRequest};

pub const TIMEOUT: Duration = Duration::from_secs(10);


/// Client for connecting to DSF daemon instances
pub struct Client {
    actor: Addr<ClientActor>,
}

impl Client {
    /// Create a new remote-hal instance
    pub fn new(addr: &str) -> Result<Self, Error> {
        debug!("client connecting to: {}", addr);
            
        let actor = ClientActor::new(addr)?;

        Ok(Self{actor})
    }

    /// Pass-through for raw requests
    pub fn request(&mut self, request: RequestKind) -> impl Future<Output=Result<ResponseKind, Error>> {
        let request = Request::new(request);
        let a = self.actor.clone();

        debug!("Sending request to actor: {:?}", request);

        Box::new(a.send(SingleRequest(request))
            .map_err(|_e| Error::None(()) )
            .and_then(|resp| {
               resp
            }))
        
    }
}

impl Create for Client {
    type Options = ();
    type Error = ();

    /// Create a new service with the provided options
    /// This MUST be stored locally for reuse
    fn create(_options: &Self::Options) -> FutureResult<ServiceHandle, Self::Error> {
        unimplemented!()
    }
}

impl Register for Client {
    type Error = ();

    /// Register a service instance in the distributed database
    fn register(&mut self, _service: &mut ServiceHandle) -> FutureResult<(), Self::Error> {
        unimplemented!()
    }
}

impl Locate for Client {
    type Error = Error;

    /// Locate a service instance in the distributed database
    /// This returns a future that will resolve to the desired service or an error
    fn locate(&mut self, id: &Id) -> FutureResult<ServiceHandle, Self::Error> {
        let req = RequestKind::Service(ServiceCommands::locate(id.clone()));
        let id = id.clone();

        Box::new(self.request(req)
            .and_then(move |resp| {
                match resp {
                    ResponseKind::Located(_info) => Ok(ServiceHandle{id}),
                    _ => Err(Error::UnrecognizedResult),
                }
            })
        )
    }
}

impl Publish for Client {
    type Error = Error;

    /// Publish data using an existing service
    fn publish(&mut self, s: &ServiceHandle, kind: Option<DataKind>, data: Option<&[u8]>) -> FutureResult<(), Self::Error> {
        let p = PublishOptions {
            service: ServiceIdentifier::id(s.id),
            kind: kind.map(|k| k.into() ),
            data: data.map(|d| d.to_vec() ),
            data_file: None,
        };
        
        let req = RequestKind::Data(DataCommands::Publish(p));

        Box::new(self.request(req)
            .and_then(move |resp| {
                match resp {
                    ResponseKind::Published(_info) => Ok(()),
                    _ => Err(Error::UnrecognizedResult),
                }
            })
        )
    }
}


pub struct SubscribeOptions{

}

impl Default for SubscribeOptions {
    fn default() -> Self {
        Self {}
    }
}

impl Subscribe for Client {
    type Options = SubscribeOptions;
    type Data = ResponseKind;
    type Error = Error;

    /// Subscribe to data from a given service
    fn subscribe(&mut self, service: &ServiceHandle, _options: Self::Options) -> FutureStream<Self::Data, Self::Error> {
        let _a = self.actor.clone();

        let s = StreamOptions{service: ServiceIdentifier::id(service.id)};
        let request = Request::new(RequestKind::Stream(s));

        debug!("Sending stream request to actor: {:?}", request);

        unimplemented!()
    }
}