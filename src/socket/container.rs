use Error;
use core::ops::{Deref, DerefMut};
use managed::ManagedSlice;
use socket::{TcpSocket, UdpSocket, RawSocket, Socket, AsSocket, TcpState};
use super::dispatch::DispatchTable;
use super::set::{Set as SocketSet, Item as SocketSetItem,
                 Handle as SocketHandle, IterMut as SetIterMut};
use wire::{IpVersion, IpProtocol, IpRepr, UdpRepr, TcpRepr, IpEndpoint};

/// A container of sockets with packet dispathing.
///
/// The lifetimes `'b` and `'c` are used when storing a `Socket<'b, 'c>`.
#[derive(Debug)]
pub struct Container<'a, 'b: 'a, 'c: 'a + 'b> {
    set: SocketSet<'a, 'b, 'c>,
    dispatch_table: DispatchTable,
}

impl<'a, 'b: 'a, 'c: 'a + 'b> Container<'a, 'b, 'c> {
    /// Create a new socket container using the provided storage
    pub fn new<SocketsT>(sockets: SocketsT) -> Container<'a, 'b, 'c>
        where SocketsT: Into<ManagedSlice<'a, Option<SocketSetItem<'b, 'c>>>>,
    {
        Container {
            set: SocketSet::new(sockets),
            dispatch_table: DispatchTable::new(),
        }
    }

    /// Add a socket to the set with the reference count 1, and return its handle.
    ///
    /// # Panics
    /// This function panics if the storage is fixed-size (not a `Vec`) and is full.
    pub fn add(&mut self, socket: Socket<'b, 'c>) -> Result<SocketHandle, Error> {
        let handle = self.set.add(socket);
        self.dispatch_table
            .add_socket(self.set.get(handle), handle)?;
        Ok(handle)
    }

    /// Get a tracked socket from the container by its handle, as mutable.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set.
    pub fn get_mut<'d, T>(&'d mut self, handle: SocketHandle) -> Option<SocketTracker<'d, T>>
        where T: TrackedSocket + 'd, Socket<'b, 'c>: AsSocket<T>
    {
        if let Some(socket) = self.set.get_mut(handle).try_as_socket() {
            Some(SocketTracker::new(&mut self.dispatch_table, handle, socket))
        } else {
            None
        }
    }

    /// Remove a socket from the container, without changing its state.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set.
    pub fn remove(&mut self, handle: SocketHandle) -> Socket<'b, 'c> {
        let socket = self.set.remove(handle);
        let res = self.dispatch_table.remove_socket(&socket, handle);
        debug_assert!(res.is_ok());
        socket
    }

    pub(crate) fn get_raw_socket<'d>(&'d mut self, ip_version: IpVersion, ip_protocol: IpProtocol)
                                     -> Option<SocketTracker<'d, RawSocket<'b, 'c>>> {
        if let Some((raw_socket, handle)) =
            self.dispatch_table.get_raw_socket(&mut self.set, ip_version, ip_protocol)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                handle,
                raw_socket,
            ))
        } else {
            None
        }
    }

    pub(crate) fn get_udp_socket<'d>(&'d mut self, ip_repr: &IpRepr, udp_repr: &UdpRepr)
                                     -> Option<SocketTracker<'d, UdpSocket<'b, 'c>>> {
        if let Some((udp_socket, handle)) =
            self.dispatch_table.get_udp_socket(&mut self.set, ip_repr, udp_repr)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                handle,
                udp_socket,
            ))
        } else {
            None
        }
    }

    pub(crate) fn get_tcp_socket<'d>(&'d mut self, ip_repr: &IpRepr, tcp_repr: &TcpRepr)
                                     -> Option<SocketTracker<'d, TcpSocket<'b>>> {
        if let Some((tcp_socket, handle)) =
            self.dispatch_table.get_tcp_socket(&mut self.set, ip_repr, tcp_repr)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                handle,
                tcp_socket,
            ))
        } else {
            None
        }
    }

    pub(crate) fn iter_mut<'d>(&'d mut self) -> SetIterMut<'d, 'b, 'c> {
        self.set.iter_mut()
    }
}

/// A trait for socket-type-depending tracking logic.
///
/// Used for upkeel of dispatching tables.
pub trait TrackedSocket {
    type State;

    fn new_state(&Self) -> Self::State;
    fn on_drop(&Self::State, &mut DispatchTable, &mut Self, SocketHandle) {}
}

impl<'a, 'b: 'a> TrackedSocket for RawSocket<'a, 'b> {
    type State = ();

    fn new_state(_: &Self) -> Self::State {
        ()
    }
}

impl<'a, 'b: 'a> TrackedSocket for UdpSocket<'a, 'b> {
    type State = IpEndpoint;

    fn new_state(udp_socket: &Self) -> Self::State {
        udp_socket.endpoint()
    }

    fn on_drop(&old_endpoint: &Self::State, dispatch_table: &mut DispatchTable,
               socket: &mut Self, handle: SocketHandle) {
        if old_endpoint != socket.endpoint() {
            if !old_endpoint.is_unbound() {
                let res = dispatch_table.remove_udp_socket(handle);
                debug_assert!(res.is_ok());
            }
            let res = dispatch_table.add_udp_socket(socket, handle);
            debug_assert!(res.is_ok());
        }
    }
}

impl<'a> TrackedSocket for TcpSocket<'a> {
    type State = TcpState;

    fn new_state(tcp_socket: &Self) -> Self::State {
        tcp_socket.state()
    }

    fn on_drop(&old_state: &Self::State, dispatch_table: &mut DispatchTable,
               socket: &mut Self, handle: SocketHandle) {
        if old_state == socket.state() {
            return;
        }

        match (old_state, socket.state()) {
            (_, TcpState::Closed) => {
                let res = dispatch_table.remove_tcp_socket(handle);
                debug_assert!(res.is_ok());
            }
            (TcpState::Closed, _) => {
                let res = dispatch_table.add_tcp_socket(socket, handle);
                debug_assert!(res.is_ok());
            }
            (TcpState::TimeWait, _) |
            (TcpState::Listen, _) => {
                let res = dispatch_table.remove_tcp_socket(handle);
                debug_assert!(res.is_ok());
                let res = dispatch_table.add_tcp_socket(socket, handle);
                debug_assert!(res.is_ok());
            }
            (_, _) => {}
        }
    }
}

/// A tracking smart-pointer to a socket.
///
/// Implements `Deref` and `DerefMut` to the socket it contains.
/// Keeps the dispatching tables up to date by updating them in `drop`.
#[derive(Debug)]
pub struct SocketTracker<'a, T: TrackedSocket + 'a> {
    handle: SocketHandle,
    socket: &'a mut T,
    dispatch_table: &'a mut DispatchTable,
    state: T::State,
}

impl<'a, T: TrackedSocket + 'a> SocketTracker<'a, T> {
    fn new(dispatch_table: &'a mut DispatchTable, handle: SocketHandle, socket: &'a mut T) -> Self {
        let state = TrackedSocket::new_state(socket);
        SocketTracker {
            handle,
            dispatch_table,
            socket,
            state,
        }

    }
}

impl<'a, T: TrackedSocket + 'a> Drop for SocketTracker<'a, T> {
    fn drop(&mut self) {
        TrackedSocket::on_drop(&self.state, self.dispatch_table, self.socket, self.handle);
    }
}

impl<'a, T: TrackedSocket + 'a> Deref for SocketTracker<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.socket
    }
}

impl<'a, T: TrackedSocket + 'a> DerefMut for SocketTracker<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.socket
    }
}

#[cfg(test)]
mod test {
    use socket::*;
    use wire::*;
    #[test]
    fn dispatcher() {
        let eps = [IpEndpoint::new(IpAddress::Unspecified, 12345u16),
                   IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 1)), 12345u16),
                   IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 2)), 12345u16),
                   IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 3)), 12345u16),
                   IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 4)), 12345u16),
                   IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 5)), 12345u16)];

        let mut sockets = SocketContainer::new(vec![]);

        let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 64])]);
        let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 128])]);
        let udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);

        let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
        let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
        let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);

        let tcp_rx_buffer2 = TcpSocketBuffer::new(vec![0; 64]);
        let tcp_tx_buffer2 = TcpSocketBuffer::new(vec![0; 128]);
        let tcp_socket2 = TcpSocket::new(tcp_rx_buffer2, tcp_tx_buffer2);

        let tcp_rx_buffer3 = TcpSocketBuffer::new(vec![0; 64]);
        let tcp_tx_buffer3 = TcpSocketBuffer::new(vec![0; 128]);
        let tcp_socket3 = TcpSocket::new(tcp_rx_buffer3, tcp_tx_buffer3);

        let udp_handle = sockets.add(udp_socket).unwrap();
        let tcp_handle = sockets.add(tcp_socket).unwrap();
        let tcp_handle2 = sockets.add(tcp_socket2).unwrap();
        let tcp_handle3 = sockets.add(tcp_socket3).unwrap();

        {
            let mut tcp_socket = sockets.get_mut::<TcpSocket>(tcp_handle).unwrap();
            tcp_socket.set_debug_id(101);
            tcp_socket.listen(eps[0]).unwrap();
        }
        {
            let mut tcp_socket2 = sockets.get_mut::<TcpSocket>(tcp_handle2).unwrap();
            tcp_socket2.set_debug_id(102);
            tcp_socket2.listen(eps[2]).unwrap();
        }
        {
            let mut tcp_socket3 = sockets.get_mut::<TcpSocket>(tcp_handle3).unwrap();
            tcp_socket3.set_debug_id(103);
            tcp_socket3.listen(eps[4]).unwrap();
        }
        {
            let mut udp_socket = sockets.get_mut::<UdpSocket>(udp_handle).unwrap();
            udp_socket.set_debug_id(201);
            udp_socket.bind(eps[0]);
        }

        let tcp_payload = vec![];
        let udp_payload = vec![];

        let mut tcp_repr = TcpRepr {
            src_port: 9999u16,
            dst_port: 12345u16,
            control: TcpControl::Syn,
            push: false,
            seq_number: TcpSeqNumber(0),
            ack_number: None,
            window_len: 0u16,
            max_seg_size: None,
            payload: &tcp_payload,
        };

        let mut ipv4_repr = Ipv4Repr {
            src_addr: Ipv4Address::new(192,168, 1, 100),
            dst_addr: Ipv4Address::new(192,168, 1, 1),
            protocol: IpProtocol::Tcp,
            payload_len: 0,
        };

        let udp_repr = UdpRepr {
            src_port: 9999u16,
            dst_port: 12345u16,
            payload: &udp_payload,
        };

        {
            let tracked_socket = sockets.get_udp_socket(&IpRepr::Ipv4(ipv4_repr), &udp_repr)
                .unwrap();
            assert_eq!(tracked_socket.debug_id(), 201);
        }

        for &(ep_index, expected_debug_id) in &[(1, 101), (2, 102), (3, 101), (4, 103), (5, 101)] {
            ipv4_repr.dst_addr = match eps[ep_index].addr {
                IpAddress::Ipv4(a) => a,
                _ => unreachable!(),
            };
            tcp_repr.dst_port = eps[ep_index].port;
            let tracked_socket = sockets.get_tcp_socket(&IpRepr::Ipv4(ipv4_repr), &tcp_repr)
                .unwrap();
            assert_eq!(tracked_socket.debug_id(), expected_debug_id);
        }

        sockets.remove(udp_handle);

        assert!(sockets.get_udp_socket(&IpRepr::Ipv4(ipv4_repr), &udp_repr).is_none());

        ipv4_repr.dst_addr = Ipv4Address::new(192, 168, 1, 2);

        assert_eq!(sockets.get_tcp_socket(&IpRepr::Ipv4(ipv4_repr), &tcp_repr).unwrap().debug_id(),
                   102);

        sockets.remove(tcp_handle2);

        assert_eq!(sockets.get_tcp_socket(&IpRepr::Ipv4(ipv4_repr), &tcp_repr).unwrap().debug_id(),
                   101);
    }
}
