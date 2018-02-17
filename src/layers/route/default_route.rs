use layers;
use layers::{IfaceDescr, ReconfigurableLayer};
use wire::IpAddress;
#[cfg(feature = "proto-ipv4")]
use wire::Ipv4Address;
#[cfg(feature = "proto-ipv6")]
use wire::Ipv6Address;
use {Error, Result};

#[derive(Debug)]
struct RouterData {
    #[cfg(feature = "proto-ipv4")]
    ipv4_gateway: Option<Ipv4Address>,
    #[cfg(feature = "proto-ipv4")]
    ipv4_gateway_iface: layers::IfaceId,
    #[cfg(feature = "proto-ipv6")]
    ipv6_gateway: Option<Ipv6Address>,
    #[cfg(feature = "proto-ipv6")]
    ipv6_gateway_iface: layers::IfaceId,
}

pub struct Emitter<'a, E> {
    router_data: &'a RouterData,
    lower: E,
}

impl<'a, E> layers::Emitter for Emitter<'a, E>
where
    E: layers::Emitter,
{
    type IfaceDescr = E::IfaceDescr;

    fn emit<F, R>(
        &mut self,
        routing_data: &layers::RoutingData,
        size_req: &layers::SizeReq,
        f: &mut F,
    ) -> Result<R>
    where
        F: FnMut(&mut [u8], &Self::IfaceDescr) -> Result<R>,
    {
        let res = self.lower.emit(routing_data, size_req, f);
        match res {
            Err(Error::Unaddressable) => {}
            _ => {
                return res;
            }
        };

        let mut rd = (*routing_data).clone();

        match rd.dst_host {
            #[cfg(feature = "proto-ipv4")]
            IpAddress::Ipv4(_) => {
                if let Some(ipv4_gateway) = self.router_data.ipv4_gateway {
                    rd.dst_host = IpAddress::Ipv4(ipv4_gateway);
                    rd.iface_id = self.router_data.ipv4_gateway_iface;
                }
            }
            #[cfg(feature = "proto-ipv6")]
            IpAddress::Ipv6(_) => {
                if let Some(ipv6_gateway) = self.router_data.ipv6_gateway {
                    rd.dst_host = IpAddress::Ipv6(ipv6_gateway);
                    rd.iface_id = self.router_data.ipv6_gateway_iface;
                }
            }
            _ => {
                return Err(Error::Unaddressable);
            }
        }

        self.lower.emit(&routing_data, size_req, f)
    }
}

#[derive(Debug)]
pub struct Layer<L> {
    lower: L,
    router_data: RouterData,
}

impl<'a, L> layers::Layer<'a> for Layer<L>
where
    L: layers::Layer<'a> + layers::IfaceCollection + 'a,
{
    type Emitter = Emitter<'a, L::Emitter>;
    type Poller = L::Poller;

    fn split(&'a mut self) -> (Self::Emitter, Self::Poller)
    {
        let (emitter, poller) = self.lower.split();
        (
            Emitter {
                lower: emitter,
                router_data: &self.router_data,
            },
            poller,
        )
    }

    fn sync(&mut self) {
        self.lower.sync()
    }
}

impl<'a, L> layers::IfaceCollection for Layer<L>
where
    L: layers::IfaceCollection + 'a,
{
    type Descr = <L as layers::IfaceCollection>::Descr;

    fn ifaces_available(&self) -> &[layers::IfaceId] {
        self.lower.ifaces_available()
    }

    fn iface(&self, iface_id: layers::IfaceId) -> Option<&Self::Descr> {
        self.lower.iface(iface_id)
    }
}

impl<'a, L> Layer<L>
where
    L: layers::Layer<'a> + layers::IfaceCollection + 'a,
{
    pub fn new(lower: L) -> Layer<L> {
        let mut layer = Layer {
            lower,
            router_data: RouterData {
                #[cfg(feature = "proto-ipv4")]
                ipv4_gateway: None,
                #[cfg(feature = "proto-ipv4")]
                ipv4_gateway_iface: layers::IFACE_ID_LOOPBACK,
                #[cfg(feature = "proto-ipv6")]
                ipv6_gateway: None,
                #[cfg(feature = "proto-ipv6")]
                ipv6_gateway_iface: layers::IFACE_ID_LOOPBACK,
            },
        };
        layer.reconfigure();
        layer
    }

    #[cfg(feature = "proto-ipv4")]
    pub fn set_ipv4_gateway(&mut self, gateway: Option<Ipv4Address>) {
        self.router_data.ipv4_gateway = gateway;
        self.reconfigure();
    }

    #[cfg(feature = "proto-ipv6")]
    pub fn set_ipv6_gateway(&mut self, gateway: Option<Ipv6Address>) {
        self.router_data.ipv6_gateway = gateway;
        self.reconfigure();
    }
}

impl<'a, L> layers::LayerWithLower for Layer<L>
where
    L: layers::Layer<'a>,
{
    type Lower = L;

    fn lower(&self) -> &Self::Lower {
        &self.lower
    }

    fn lower_mut(&mut self) -> &mut Self::Lower {
        &mut self.lower
    }
}

impl<'a, L> layers::ReconfigurableLayer for Layer<L>
where
    L: layers::Layer<'a> + layers::IfaceCollection,
{
    fn reconfigure(&mut self) {
        #[cfg(feature = "proto-ipv4")]
        {
            self.router_data.ipv4_gateway_iface = layers::IFACE_ID_UNKNOWN;
        }
        #[cfg(feature = "proto-ipv6")]
        {
            self.router_data.ipv6_gateway_iface = layers::IFACE_ID_UNKNOWN;
        }
        for iface in self.lower.ifaces_available() {
            let descr = match self.lower.iface(*iface) {
                Some(descr) => descr,
                None => continue,
            };
            for cidr in descr.addrs() {
                #[cfg(feature = "proto-ipv4")]
                {
                    if let Some(ipv4_gateway) = self.router_data.ipv4_gateway {
                        if cidr.contains_addr(&IpAddress::Ipv4(ipv4_gateway)) {
                            self.router_data.ipv4_gateway_iface = *iface;
                        }
                    }
                }
                #[cfg(feature = "proto-ipv6")]
                {
                    if let Some(ipv6_gateway) = self.router_data.ipv6_gateway {
                        if cidr.contains_addr(&IpAddress::Ipv6(ipv6_gateway)) {
                            self.router_data.ipv6_gateway_iface = *iface;
                        }
                    }
                }
            }
        }
    }
}
