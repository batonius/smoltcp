use Error;
use socket::set::{Set as SocketSet};
use socket::{SocketHandle, TcpSocket, UdpSocket, RawSocket, Socket, AsSocket};
use std::collections::btree_map::Entry as MapEntry;
use std::collections::{BTreeMap, BTreeSet};
use wire::{IpVersion, IpProtocol, IpEndpoint, IpAddress, IpRepr, UdpRepr, TcpRepr};

#[derive(Debug)]
struct TcpLocalEndpoint {
    listen_sockets: BTreeSet<SocketHandle>,
    established_sockets: BTreeMap<IpEndpoint, SocketHandle>,
}

impl TcpLocalEndpoint {
    pub fn new() -> TcpLocalEndpoint {
        TcpLocalEndpoint {
            listen_sockets: BTreeSet::new(),
            established_sockets: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct DispatchTable {
    raw: BTreeMap<(IpVersion, IpProtocol), SocketHandle>,
    udp: BTreeMap<IpEndpoint, SocketHandle>,
    tcp: BTreeMap<IpEndpoint, TcpLocalEndpoint>,

    rev_raw: BTreeMap<SocketHandle, (IpVersion, IpProtocol)>,
    rev_udp: BTreeMap<SocketHandle, IpEndpoint>,
    rev_tcp: BTreeMap<SocketHandle, (IpEndpoint, IpEndpoint)>,
}

pub type WithHandle<'a, T> = Option<(&'a mut T, SocketHandle)>;

impl DispatchTable {
    pub(crate) fn new() -> DispatchTable {
        DispatchTable {
            raw: BTreeMap::new(),
            tcp: BTreeMap::new(),
            udp: BTreeMap::new(),
            rev_raw: BTreeMap::new(),
            rev_tcp: BTreeMap::new(),
            rev_udp: BTreeMap::new(),
        }
    }

    pub(crate) fn add_socket(&mut self, socket: &Socket, handle: SocketHandle) -> Result<(), Error> {
        match *socket {
            Socket::Udp(ref udp_socket) => self.add_udp_socket(udp_socket, handle),
            Socket::Tcp(ref tcp_socket) => self.add_tcp_socket(tcp_socket, handle),
            Socket::Raw(ref raw_socket) => self.add_raw_socket(raw_socket, handle),
            _ => unreachable!(),
        }
    }

    pub(crate) fn add_udp_socket(&mut self, udp_socket: &UdpSocket, handle: SocketHandle)
                                 -> Result<(), Error> {
        if udp_socket.endpoint().is_unbound() {
            return Ok(());
        }
        match (self.udp.entry(udp_socket.endpoint()), self.rev_udp.entry(handle)) {
            (MapEntry::Vacant(e), MapEntry::Vacant(re)) => {
                e.insert(handle);
                re.insert(udp_socket.endpoint());
            }
            _ => return Err(Error::AlreadyInUse),
        };
        Ok(())
    }

    pub(crate) fn add_raw_socket(&mut self, raw_socket: &RawSocket, handle: SocketHandle)
                                 -> Result<(), Error> {
        let key = (raw_socket.ip_version(), raw_socket.ip_protocol());
        match (self.raw.entry(key), self.rev_raw.entry(handle)) {
            (MapEntry::Vacant(e), MapEntry::Vacant(re)) => {
                e.insert(handle);
                re.insert(key);
            }
            _ => return Err(Error::AlreadyInUse),
        };
        Ok(())
    }

    pub(crate) fn add_tcp_socket(&mut self, tcp_socket: &TcpSocket, handle: SocketHandle)
                                 -> Result<(), Error> {
        if tcp_socket.local_endpoint().is_unbound() {
            return Ok(());
        }

        let rev_entry = match self.rev_tcp.entry(handle) {
            MapEntry::Occupied(_) => return Err(Error::AlreadyInUse),
            MapEntry::Vacant(e) => e,
        };

        let rev_key = (tcp_socket.local_endpoint(), tcp_socket.remote_endpoint());
        let tcp_endpoint = self.tcp
            .entry(tcp_socket.local_endpoint())
            .or_insert_with(TcpLocalEndpoint::new);

        if tcp_socket.remote_endpoint().is_unbound() {
            tcp_endpoint.listen_sockets.insert(handle);
            rev_entry.insert(rev_key);
        } else {
            match tcp_endpoint.established_sockets
                .entry(tcp_socket.remote_endpoint()) {
                MapEntry::Occupied(_) => return Err(Error::AlreadyInUse),
                MapEntry::Vacant(e) => {
                    e.insert(handle);
                    rev_entry.insert(rev_key);
                }
            };
        }
        Ok(())
    }

    pub(crate) fn remove_socket(&mut self, socket: &Socket, handle: SocketHandle)
                                -> Result<(), Error> {
        match *socket {
            Socket::Udp(_) => self.remove_udp_socket(handle),
            Socket::Tcp(_) => self.remove_tcp_socket(handle),
            Socket::Raw(_) => self.remove_raw_socket(handle),
            _ => unreachable!(),
        }
    }

    pub(crate) fn remove_udp_socket(&mut self, handle: SocketHandle) -> Result<(), Error> {
        match self.rev_udp.entry(handle) {
            MapEntry::Vacant(_) => Err(Error::SocketNotFound),
            MapEntry::Occupied(re) => {
                match self.udp.entry(*re.get()) {
                    MapEntry::Vacant(_) => Err(Error::SocketNotFound),
                    MapEntry::Occupied(e) => {
                        e.remove();
                        re.remove();
                        Ok(())
                    }
                }
            }
        }
    }

    pub(crate) fn remove_raw_socket(&mut self, handle: SocketHandle) -> Result<(), Error> {
        match self.rev_raw.entry(handle) {
            MapEntry::Vacant(_) => Err(Error::SocketNotFound),
            MapEntry::Occupied(re) => {
                match self.raw.entry(*re.get()) {
                    MapEntry::Vacant(_) => Err(Error::SocketNotFound),
                    MapEntry::Occupied(e) => {
                        e.remove();
                        re.remove();
                        Ok(())
                    }
                }
            }
        }
    }

    pub(crate) fn remove_tcp_socket(&mut self, handle: SocketHandle) -> Result<(), Error> {
        let re = match self.rev_tcp.entry(handle) {
            MapEntry::Vacant(_) => return Err(Error::SocketNotFound),
            MapEntry::Occupied(re) => re,
        };

        let &(local_endpoint, remote_endpoint) = re.get();

        let mut tc_endpoint_entry = match self.tcp.entry(local_endpoint) {
            MapEntry::Vacant(_) => return Err(Error::SocketNotFound),
            MapEntry::Occupied(e) => e,
        };

        {
            let mut tcp_endpoint = tc_endpoint_entry.get_mut();

            if remote_endpoint.is_unbound() {
                if tcp_endpoint.listen_sockets.remove(&handle) {
                    re.remove();
                } else {
                    return Err(Error::SocketNotFound);
                }
            } else {
                match tcp_endpoint.established_sockets.entry(remote_endpoint) {
                    MapEntry::Occupied(o) => {
                        o.remove();
                        re.remove();
                    }
                    MapEntry::Vacant(_) => return Err(Error::SocketNotFound),
                }
            }
        }

        if tc_endpoint_entry.get().listen_sockets.is_empty() &&
            tc_endpoint_entry.get().established_sockets.is_empty()
        {
            tc_endpoint_entry.remove();
        }

        Ok(())
    }

    pub(crate) fn get_raw_socket<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self, set: &'d mut SocketSet<'a, 'b, 'c>, ip_version: IpVersion, ip_protocol: IpProtocol)
        -> WithHandle<'d, RawSocket<'b, 'c>> {
        let key = (ip_version, ip_protocol);
        self.raw
            .get(&key)
            .map(move |handle| (set.get_mut(*handle), handle))
            .and_then(|(s, &h)| s.try_as_socket().map(|s| (s, h)))
    }

    pub(crate) fn get_udp_socket<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self, set: &'d mut SocketSet<'a, 'b, 'c>, ip_repr: &IpRepr, udp_repr: &UdpRepr)
        -> WithHandle<'d, UdpSocket<'b, 'c>> {
        DispatchTable::get_socket_data(
            &self.udp,
            IpEndpoint::new(ip_repr.dst_addr(), udp_repr.dst_port),
        ).map(move |handle| (set.get_mut(*handle), handle))
            .and_then(|(s, &h)| s.try_as_socket().map(|s| (s, h)))
    }

    pub(crate) fn get_tcp_socket<'a, 'b: 'a, 'c: 'a + 'b, 'd>(
        &self, set: &'d mut SocketSet<'a, 'b, 'c>, ip_repr: &IpRepr, tcp_repr: &TcpRepr)
        -> WithHandle<'d, TcpSocket<'b>> {
        DispatchTable::get_socket_data(
            &self.tcp,
            IpEndpoint::new(ip_repr.dst_addr(), tcp_repr.dst_port),
        ).and_then(|tcp_endpoint| {
            tcp_endpoint
                .established_sockets
                .get(&IpEndpoint::new(ip_repr.src_addr(), tcp_repr.src_port))
                .or_else(|| tcp_endpoint.listen_sockets.iter().next())
        }).map(move |handle| (set.get_mut(*handle), handle))
            .and_then(|(s, &h)| s.try_as_socket().map(|s| (s, h)))
    }

    fn get_socket_data<T>(tree: &BTreeMap<IpEndpoint, T>, endpoint: IpEndpoint) -> Option<&T> {
        use std::collections::Bound::Included;
        let mut unspecified_endpoint = endpoint;
        unspecified_endpoint.addr = IpAddress::Unspecified;
        let mut range = tree.range((Included(unspecified_endpoint), Included(endpoint)));
        match range.next_back() {
            None => None,
            Some((&IpEndpoint { ref addr, .. }, h))
                if *addr == endpoint.addr || addr.is_unspecified() => Some(h),
            _ => {
                match range.next() {
                    Some((e, h)) if e.is_unbound() => Some(h),
                    _ => None,
                }
            }
        }
    }
}
