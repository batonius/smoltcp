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
