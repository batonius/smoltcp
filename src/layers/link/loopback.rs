use layers;
use layers::SizeReq;
use phy::DeviceCapabilities;
use std::vec::Vec;
use wire::{IpAddress, IpCidr, IpVersion};
#[cfg(feature = "proto-ipv4")]
use wire::{Ipv4Address, Ipv4Cidr, Ipv4Packet};
#[cfg(feature = "proto-ipv6")]
use wire::{Ipv6Address, Ipv6Cidr, Ipv6Packet};
use {Error, Result};

#[derive(Debug)]
struct Packet {
    ip_version: IpVersion,
    data: Vec<u8>,
}

#[derive(Debug)]
pub struct IfaceDescr {
    dev_caps: DeviceCapabilities,
    #[cfg(all(feature = "proto-ipv4", feature = "proto-ipv6"))]
    addrs: [IpCidr; 2],
    #[cfg(
        any(
            all(feature = "proto-ipv4", not(feature = "proto-ipv6")),
            all(not(feature = "proto-ipv4"), feature = "proto-ipv6")
        )
    )]
    addrs: [IpCidr; 1],
}

impl layers::IfaceDescr for IfaceDescr {
    fn dev_caps(&self) -> &DeviceCapabilities {
        &self.dev_caps
    }

    fn addrs(&self) -> &[IpCidr] {
        &self.addrs
    }
}

pub struct Emitter<'a> {
    out_queue: &'a mut Vec<Packet>,
    iface_descr: &'a IfaceDescr,
}

impl<'a> layers::Emitter for Emitter<'a> {
    type IfaceDescr = IfaceDescr;

    fn emit<F, R>(
        &mut self,
        routing_data: &layers::RoutingData,
        size_req: &SizeReq,
        f: &mut F,
    ) -> Result<R>
    where
        F: FnMut(&mut [u8], &Self::IfaceDescr) -> Result<R>,
    {
        if !routing_data.is_iface_unknown() && !routing_data.is_iface_loopback() {
            return Err(Error::Unaddressable);
        }

        let ip_version = match routing_data.dst_host {
            #[cfg(feature = "proto-ipv4")]
            IpAddress::Ipv4(ip) => {
                if !ip.is_loopback() {
                    return Err(Error::Unaddressable);
                }
                IpVersion::Ipv4
            }
            #[cfg(feature = "proto-ipv6")]
            IpAddress::Ipv6(ip) => {
                if !ip.is_loopback() {
                    return Err(Error::Unaddressable);
                }
                IpVersion::Ipv6
            }
            _ => {
                return Err(Error::Unaddressable);
            }
        };
        let optimal_size = size_req.optimal_size(self.iface_descr.dev_caps.max_transmission_unit);
        let optimal_size = optimal_size.ok_or(Error::Truncated)?;
        let mut data = vec![0; optimal_size];
        let result = f(data.as_mut(), self.iface_descr)?;
        self.out_queue.push(Packet { ip_version, data });
        Ok(result)
    }
}

pub struct Poller<'a> {
    in_queue: &'a mut Vec<Packet>,
}

impl<'a> Poller<'a> {
    fn new(in_queue: &'a mut Vec<Packet>) -> Poller<'a> {
        Poller { in_queue }
    }
}

impl<'a> layers::Poller for Poller<'a> {
    fn poll<F, R>(&mut self, mut f: F) -> Result<R>
    where
        F: FnMut(layers::IfaceId, layers::IpPacket) -> R,
    {
        if let Some(packet) = self.in_queue.pop() {
            let packet = match packet.ip_version {
                #[cfg(feature = "proto-ipv4")]
                IpVersion::Ipv4 => {
                    layers::IpPacket::Ipv4(Ipv4Packet::new_checked(packet.data.as_slice())?)
                }
                #[cfg(feature = "proto-ipv6")]
                IpVersion::Ipv6 => {
                    layers::IpPacket::Ipv6(Ipv6Packet::new_checked(packet.data.as_slice())?)
                }
                _ => unreachable!(),
            };
            Ok(f(layers::IFACE_ID_LOOPBACK, packet))
        } else {
            Err(Error::Exhausted)
        }
    }
}

#[derive(Debug)]
pub struct Layer {
    iface_descr: IfaceDescr,
    iface_id: [layers::IfaceId; 1],
    in_queue: Vec<Packet>,
    out_queue: Vec<Packet>,
}

impl Layer {
    pub fn new() -> Layer {
        let mut dev_caps = DeviceCapabilities::default();
        dev_caps.max_transmission_unit = 1500;
        Layer {
            iface_descr: IfaceDescr {
                dev_caps,
                #[cfg(all(feature = "proto-ipv4", not(feature = "proto-ipv6")))]
                addrs: [IpCidr::Ipv4(Ipv4Cidr::new(
                    Ipv4Address::new(127, 0, 0, 1),
                    8,
                ))],
                #[cfg(all(feature = "proto-ipv6", not(feature = "proto-ipv4")))]
                addrs: [IpCidr::Ipv6(Ipv6Cidr::new(
                    Ipv6Address::new(0, 0, 0, 0, 0, 0, 0, 1),
                    128,
                ))],
                #[cfg(all(feature = "proto-ipv4", feature = "proto-ipv6"))]
                addrs: [
                    IpCidr::Ipv4(Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8)),
                    IpCidr::Ipv6(Ipv6Cidr::new(Ipv6Address::new(0, 0, 0, 0, 0, 0, 0, 1), 128)),
                ],
            },
            iface_id: [layers::IFACE_ID_LOOPBACK],
            in_queue: vec![],
            out_queue: vec![],
        }
    }
}

impl layers::IfaceCollection for Layer {
    type Descr = IfaceDescr;

    fn ifaces_available(&self) -> &[layers::IfaceId] {
        &self.iface_id
    }

    fn iface(&self, iface_id: layers::IfaceId) -> Option<&Self::Descr> {
        if iface_id == layers::IFACE_ID_LOOPBACK {
            Some(&self.iface_descr)
        } else {
            None
        }
    }
}

impl<'a> layers::Layer<'a> for Layer {
    type Emitter = Emitter<'a>;
    type Poller = Poller<'a>;

    fn split(&'a mut self) -> (Self::Emitter, Self::Poller)
    {
        (
            Emitter {
                out_queue: &mut self.out_queue,
                iface_descr: &self.iface_descr,
            },
            Poller::new(&mut self.in_queue),
        )
    }

    fn sync(&mut self) {
        if self.in_queue.is_empty() {
            ::std::mem::swap(&mut self.in_queue, &mut self.out_queue);
        }
    }
}
