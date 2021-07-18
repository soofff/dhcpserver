use std::net::{UdpSocket, Ipv4Addr, SocketAddr, SocketAddrV4};
use crate::config::DhcpConfig;
use crate::error::{DhcpResult, DhcpError};
use dhcplib::option::{DhcpOption, DhcpOptions, BOOT_FILE_NAME, MESSAGE, IP_ADDRESS_LEASE_TIME, VENDOR_CLASS_IDENTIFIER, SERVER_IDENTIFIER};
use dhcplib::messaging::DhcpMessaging;
use dhcplib::DhcpPacket;
use tokio::sync::Mutex;
use std::sync::Arc;
use pnet::ipnetwork::{IpNetwork, Ipv4Network};
use crate::sources::DhcpHostSource;
use std::convert::TryFrom;

const UDP_PACKET_BUFFER_SIZE: usize = 512;

pub struct Server {}

impl Server {
    pub async fn listen(mut config: DhcpConfig) -> DhcpResult<()> {
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, config.port()))?;
        socket.set_broadcast(true)?;

        log::info!("UDP Socket bound on port {}", config.port());

        let mut buf = vec![0u8; UDP_PACKET_BUFFER_SIZE];
        let sources = config.init_sources()?;
        let shared_source = Arc::new(Mutex::new(sources));

        // prepare available networks
        let local_networks = pnet::datalink::interfaces().iter().map(|i| {
            i.ips.iter().filter_map(|ip| {
                if let IpNetwork::V4(i) = ip {
                    match config.ips() {
                        None => Some(*i),
                        Some(ips) if ips.contains(&i.ip()) => {
                            Some(*i)
                        }
                        _ => None,
                    }
                } else { None }
            }).collect::<Vec<Ipv4Network>>()
        }).flatten().collect::<Vec<Ipv4Network>>();

        log::debug!("Outbound ip addresses: {:?}", local_networks.iter().map(|i| i.ip()).collect::<Vec<Ipv4Addr>>());

        loop {
            let (_, sender) = socket.recv_from(&mut buf)?;

            log::trace!("UDP packet received");

            let bytes = buf.clone();
            let cloned_source = shared_source.clone();
            let cloned_socket = socket.try_clone()?;
            let cloned_local_networks = local_networks.clone();

            match tokio::spawn(async move {
                log::trace!("spawning new thread");
                Self::process(bytes, cloned_source, sender, cloned_socket, cloned_local_networks).await
            }).await {
                Ok(_) => {}
                Err(e) => log::error!("{:?}", e)
            }
        }
    }

    fn send(p: DhcpPacket, socket: UdpSocket, mut sender: SocketAddr, local_networks: Vec<Ipv4Network>) -> DhcpResult<()> {
        let mut bytes = p.into_bytes_with_server_ips(local_networks.iter().map(|s| s.ip()).collect());

        for a in local_networks {
            sender.set_ip(a.broadcast().into());
            if let Some(b) = bytes.remove(&a.ip()) {
                socket.send_to(b.as_slice(), sender)?;
            }
        }

        Ok(())
    }

    async fn process(bytes: Vec<u8>,
                     sources: Arc<Mutex<Vec<impl DhcpHostSource + Send>>>,
                     sender: SocketAddr,
                     socket: UdpSocket,
                     local_networks: Vec<Ipv4Network>,
    ) -> DhcpResult<()> {
        let message = DhcpMessaging::try_from(bytes.as_slice())?;
        if let Some(DhcpOption::MessageType(t)) = message.packet().message_type() {
            log::debug!("{:?} packet received", t);
        }

        match message {
            DhcpMessaging::Discover(p) => {
                for source in sources.lock().await.iter_mut() {
                    source.packet_received(p.packet()).await?;

                    match source.offer(&p.packet()).await {
                        Ok(Some(result)) => {
                            let mac = (*p.packet().client_hardware()).into();
                            let client_ip_address = result.client_ip_address().ok_or(DhcpError::ClientIpAddressMissing(mac))?;
                            let options: DhcpOptions = result.into();
                            let send_packet = p.into_offer(options.try_u32_option(IP_ADDRESS_LEASE_TIME)?,
                                                           client_ip_address,
                                                           Ipv4Addr::UNSPECIFIED,
                                                           options.try_ascii_option(BOOT_FILE_NAME).ok(),
                                                           options.try_ascii_option(MESSAGE).ok(),
                                                           options).into();

                            source.packet_sending(&send_packet).await?;
                            Self::send(send_packet, socket, sender, local_networks)?;
                            source.packet_sent().await?;
                            break;
                        }
                        Ok(None) => log::debug!("{} not found in source {}", p.packet().client_hardware(), source.name()),
                        Err(e) => log::error!("{}", e),
                    }
                }
            }
            DhcpMessaging::Offer(_) => log::trace!("offer packet discarded"),
            DhcpMessaging::Request(p) => {
                for source in sources.lock().await.iter_mut() {
                    source.packet_received(p.packet()).await?;

                    match source.reserve(&p.packet()).await {
                        Ok(Some(result)) => {
                            let mac = (*p.packet().client_hardware()).into();
                            let client_ip_address = result.client_ip_address().ok_or(DhcpError::ClientIpAddressMissing(mac))?;
                            let options: DhcpOptions = result.into();
                            let send_packet = p.into_ack(options.try_u32_option(IP_ADDRESS_LEASE_TIME)?,
                                                         client_ip_address,
                                                         Ipv4Addr::UNSPECIFIED,
                                                         options.try_ascii_option(BOOT_FILE_NAME).ok(),
                                                         options.try_ascii_option(SERVER_IDENTIFIER).ok(),
                                                         options.try_ascii_option(MESSAGE).ok(),
                                                         options.try_vec_u8_option(VENDOR_CLASS_IDENTIFIER).ok(),
                                                         options).into();

                            log::debug!("sending ack");
                            source.packet_sending(&send_packet).await?;
                            Self::send(send_packet, socket, sender, local_networks)?;
                            source.packet_sent().await?;
                            return Ok(());
                        }
                        Ok(None) => log::debug!("{} not found in source {}", p.packet().client_hardware(), source.name()),
                        Err(e) => log::error!("{}", e),
                    }
                }

                log::debug!("sending nak");
                let send_packet: DhcpPacket = p.into_nak(
                    Ipv4Addr::UNSPECIFIED,
                    None,
                    None,
                    None,
                ).into();
                Self::send(send_packet, socket, sender, local_networks)?;
            }
            DhcpMessaging::Inform(p) => {
                for source in sources.lock().await.iter_mut() {
                    source.packet_received(p.packet()).await?;

                    match source.inform(&p.packet()).await {
                        Ok(Some(result)) => {
                            let mac = (*p.packet().client_hardware()).into();
                            let client_ip_address = result.client_ip_address().ok_or(DhcpError::ClientIpAddressMissing(mac))?;
                            let options: DhcpOptions = result.into();
                            let send_packet = p.into_ack(client_ip_address,
                                                         Ipv4Addr::UNSPECIFIED,
                                                         options.try_ascii_option(BOOT_FILE_NAME).ok(),
                                                         options.try_ascii_option(SERVER_IDENTIFIER).ok(),
                                                         options.try_ascii_option(MESSAGE).ok(),
                                                         options.try_vec_u8_option(VENDOR_CLASS_IDENTIFIER).ok(),
                                                         options).into();

                            log::debug!("sending ack");
                            source.packet_sending(&send_packet).await?;
                            Self::send(send_packet, socket, sender, local_networks)?;
                            source.packet_sent().await?;
                            return Ok(());
                        }
                        Ok(None) => log::debug!("{} not found in source {}", p.packet().client_hardware(), source.name()),
                        Err(e) => log::error!("{}", e),
                    }
                }
            }
            DhcpMessaging::Release(p) => {
                for source in sources.lock().await.iter_mut() {
                    source.packet_received(p.packet()).await?;
                    source.release(&p.packet()).await?;
                }
            }
            DhcpMessaging::Decline(p) => {
                for source in sources.lock().await.iter_mut() {
                    source.packet_received(p.packet()).await?;
                    source.decline(&p.packet()).await?;
                }
            }
            DhcpMessaging::Ack(_) => log::trace!("ack packet discarded"),
            DhcpMessaging::Nak(_) => log::trace!("nak packet discarded"),
        }
        Ok(())
    }
}
