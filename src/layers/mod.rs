use phy::DeviceCapabilities;
#[cfg(feature = "proto-ipv4")]
use wire::Ipv4Packet;
#[cfg(feature = "proto-ipv6")]
use wire::Ipv6Packet;
use wire::{IpAddress, IpCidr};

use Result;

pub mod link;
pub mod route;
pub mod network;
mod size_req;

pub use self::size_req::SizeReq;

#[repr(packed)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct IfaceId {
    iface_class: u16,
    iface_no: u16,
}

const IFACE_CLASS_UNKNOWN: u16 = 1;
const IFACE_CLASS_LOOPBACK: u16 = 1;

pub const IFACE_ID_UNKNOWN: IfaceId = IfaceId {
    iface_class: IFACE_CLASS_UNKNOWN,
    iface_no: 0,
};
pub const IFACE_ID_LOOPBACK: IfaceId = IfaceId {
    iface_class: IFACE_CLASS_LOOPBACK,
    iface_no: 0,
};

#[derive(Clone, PartialEq, PartialOrd, Eq, Ord, Debug)]
pub struct RoutingData {
    pub iface_id: IfaceId,
    pub dst_host: IpAddress,
}

impl RoutingData {
    pub fn unknown_iface(dst_host: impl Into<IpAddress>) -> RoutingData {
        RoutingData {
            iface_id: IFACE_ID_UNKNOWN,
            dst_host: dst_host.into(),
        }
    }

    pub fn is_iface_unknown(&self) -> bool {
        self.iface_id.iface_class == IFACE_CLASS_UNKNOWN
    }

    pub fn is_iface_loopback(&self) -> bool {
        self.iface_id.iface_class == IFACE_CLASS_LOOPBACK
    }
}

#[derive(Debug)]
pub enum IpPacket<'a> {
    #[cfg(feature = "proto-ipv4")]
    Ipv4(Ipv4Packet<&'a [u8]>),
    #[cfg(feature = "proto-ipv6")]
    Ipv6(Ipv6Packet<&'a [u8]>),
}

pub trait IfaceDescr {
    fn dev_caps(&self) -> &DeviceCapabilities;
    fn addrs(&self) -> &[IpCidr];
}

pub trait IfaceCollection {
    type Descr: IfaceDescr;

    fn ifaces_available(&self) -> &[IfaceId];
    fn iface(&self, IfaceId) -> Option<&Self::Descr>;
}

pub trait Emitter {
    type IfaceDescr: IfaceDescr;

    fn emit<F, R>(&mut self, &RoutingData, &SizeReq, &mut F) -> Result<R>
    where
        F: FnMut(&mut [u8], &Self::IfaceDescr) -> Result<R>;
}

pub trait Poller {
    fn poll<F, R>(&mut self, F) -> Result<R>
    where
        F: FnMut(IfaceId, IpPacket) -> R;
}

pub trait Layer<'a> {
    type Emitter: Emitter + 'a;
    type Poller: Poller + 'a;

    fn split(&'a mut self) -> (Self::Emitter, Self::Poller);

    fn sync(&mut self);
}

pub trait LayerWithLower {
    type Lower;

    fn lower(&self) -> &Self::Lower;

    fn lower_mut(&mut self) -> &mut Self::Lower;
}

pub trait ReconfigurableLayer: LayerWithLower + Sized {
    fn reconfigure(&mut self);

    fn lower_tracked<'a>(&'a mut self) -> TrackedLower<'a, Self> {
        TrackedLower { layer: self }
    }
}

pub struct TrackedLower<'a, T: 'a>
where
    T: ReconfigurableLayer,
{
    layer: &'a mut T,
}

impl<'a, T> AsMut<T::Lower> for TrackedLower<'a, T>
where
    T: ReconfigurableLayer,
{
    fn as_mut(&mut self) -> &mut T::Lower {
        self.layer.lower_mut()
    }
}

impl<'a, T> Drop for TrackedLower<'a, T>
where
    T: ReconfigurableLayer,
{
    fn drop(&mut self) {
        self.layer.reconfigure();
    }
}
