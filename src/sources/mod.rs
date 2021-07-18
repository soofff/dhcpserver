use serde::Deserializer;
use crate::error::DhcpResult;
use std::net::Ipv4Addr;
use dhcplib::DhcpPacket;
use dhcplib::option::DhcpOptions;

pub mod rest;

#[derive(Debug)]
pub struct DhcpSourceResult {
    client_ip_address: Option<Ipv4Addr>,
    options: DhcpOptions
}

impl DhcpSourceResult {
    pub fn new(client_ip_address: Option<Ipv4Addr>, options: DhcpOptions) -> Self {
        Self {
            client_ip_address,
            options
        }
    }

    pub fn client_ip_address(&self) -> &Option<Ipv4Addr> { &self.client_ip_address }

    pub fn options(&self) -> &DhcpOptions { &self.options }
}

impl From<DhcpSourceResult> for DhcpOptions {
    fn from(e: DhcpSourceResult) -> Self {
        e.options
    }
}

#[async_trait::async_trait]
pub trait DhcpHostSource {
    const NAME: &'static str;

    fn name(&self) -> &'static str {
        Self::NAME
    }

    async fn offer(&mut self, p: &DhcpPacket) ->  DhcpResult<Option<DhcpSourceResult>>; // from discover --> offer

    async fn reserve(&mut self, p: &DhcpPacket) ->  DhcpResult<Option<DhcpSourceResult>>; // from request -> ack/nak

    async fn release(&mut self, p: &DhcpPacket) ->  DhcpResult<()>; // from release // release

    async fn decline(&mut self, p: &DhcpPacket) ->  DhcpResult<()>; // from release // release

    async fn inform(&mut self, p: &DhcpPacket) ->  DhcpResult<Option<DhcpSourceResult>>; // from release // release -> ack/nak

    fn from_config<'a, T: Deserializer<'a> + Send>(config: T) -> DhcpResult<Self> where Self: Sized;

    async fn packet_received(&mut self, _: &DhcpPacket) -> DhcpResult<()> { Ok(()) }

    async fn packet_sending(&mut self, _: &DhcpPacket) -> DhcpResult<()> { Ok(()) }

    async fn packet_sent(&mut self) -> DhcpResult<()> { Ok(()) }
}
