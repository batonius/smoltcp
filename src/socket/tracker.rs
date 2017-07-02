use core::ops::{Deref, DerefMut};
use socket::dispatch::DispatchTable;
use socket::set::{Handle as SocketHandle};
use socket::{TcpSocket, UdpSocket, RawSocket, TcpState, Socket};
use storage::{RingBuffer};
use wire::{IpEndpoint};

/// A trait for socket-type-depending tracking logic.
///
/// Used for upkeep of dispatching tables.
pub trait TrackedSocket {
    type State;

    fn new_state(&Self) -> Self::State;
    fn on_drop(&Self::State, &mut DispatchTable, &mut Self, SocketHandle) {}

    fn is_dirty(&Self) -> bool;
    fn is_on_dirty_list(&Self) -> bool;
    fn set_on_dirty_list(&mut Self, bool);
}

impl<'a, 'b: 'a> TrackedSocket for RawSocket<'a, 'b> {
    type State = ();

    fn new_state(_: &Self) -> Self::State {
        ()
    }

    fn is_dirty(socket: &Self) -> bool {
        socket.is_dirty()
    }

    fn is_on_dirty_list(socket: &Self) -> bool {
        socket.is_on_dirty_list()
    }

    fn set_on_dirty_list(socket: &mut Self, val: bool) {
        socket.set_on_dirty_list(val)
    }
}

impl<'a, 'b: 'a> TrackedSocket for UdpSocket<'a, 'b> {
    type State = IpEndpoint;

    fn new_state(udp_socket: &Self) -> Self::State {
        udp_socket.endpoint()
    }

    fn on_drop(&old_endpoint: &Self::State,
               dispatch_table: &mut DispatchTable,
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

    fn is_dirty(socket: &Self) -> bool {
        socket.is_dirty()
    }

    fn is_on_dirty_list(socket: &Self) -> bool {
        socket.is_on_dirty_list()
    }

    fn set_on_dirty_list(socket: &mut Self, val: bool) {
        socket.set_on_dirty_list(val)
    }
}

impl<'a> TrackedSocket for TcpSocket<'a> {
    type State = TcpState;

    fn new_state(tcp_socket: &Self) -> Self::State {
        tcp_socket.state()
    }

    fn on_drop(&old_state: &Self::State,
               dispatch_table: &mut DispatchTable,
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

    fn is_dirty(socket: &Self) -> bool {
        socket.is_dirty()
    }

    fn is_on_dirty_list(socket: &Self) -> bool {
        socket.is_on_dirty_list()
    }

    fn set_on_dirty_list(socket: &mut Self, val: bool) {
        socket.set_on_dirty_list(val)
    }
}

pub enum SocketState<'a, 'b: 'a> {
    Raw(<RawSocket<'a, 'b> as TrackedSocket>::State),
    Udp(<UdpSocket<'a, 'b> as TrackedSocket>::State),
    Tcp(<TcpSocket<'a> as TrackedSocket>::State),
}

impl<'a, 'b: 'a> TrackedSocket for Socket<'a, 'b> {
    type State = SocketState<'a, 'b>;

    fn new_state(socket: &Self) -> Self::State {
        match *socket {
            Socket::Raw(ref raw_socket) =>
                SocketState::Raw(<RawSocket as TrackedSocket>::new_state(raw_socket)),
            Socket::Udp(ref udp_socket) =>
                SocketState::Udp(<UdpSocket as TrackedSocket>::new_state(udp_socket)),
            Socket::Tcp(ref tcp_socket) =>
                SocketState::Tcp(<TcpSocket as TrackedSocket>::new_state(tcp_socket)),
            _ => unreachable!(),
        }
    }

    fn on_drop(state: &Self::State, dispatch_table: &mut DispatchTable,
               socket: &mut Self, handle: SocketHandle) {
        match (state, socket) {
            (&SocketState::Raw(ref raw_state), &mut Socket::Raw(ref mut raw_socket)) =>
                <RawSocket as TrackedSocket>::on_drop(raw_state, dispatch_table,
                                                      raw_socket, handle),
            (&SocketState::Udp(ref udp_state), &mut Socket::Udp(ref mut udp_socket)) =>
                <UdpSocket as TrackedSocket>::on_drop(udp_state, dispatch_table,
                                                      udp_socket, handle),
            (&SocketState::Tcp(ref tcp_state), &mut Socket::Tcp(ref mut tcp_socket)) =>
                <TcpSocket as TrackedSocket>::on_drop(tcp_state, dispatch_table,
                                                      tcp_socket, handle),
            _ => unreachable!(),
        }
    }

    fn is_dirty(socket: &Self) -> bool {
        socket.is_dirty()
    }

    fn is_on_dirty_list(socket: &Self) -> bool {
        socket.is_on_dirty_list()
    }

    fn set_on_dirty_list(socket: &mut Self, val: bool) {
        socket.set_on_dirty_list(val)
    }
}

/// A tracking smart-pointer to a socket.
///
/// Implements `Deref` and `DerefMut` to the socket it contains.
/// Keeps the dispatching tables up to date by updating them in `drop`.
#[derive(Debug)]
pub struct SocketTracker<'a, 'b: 'a, T: TrackedSocket + 'a> {
    handle: SocketHandle,
    socket: &'a mut T,
    dispatch_table: &'a mut DispatchTable,
    dirty_sockets: &'a mut RingBuffer<'b, SocketHandle>,
    state: T::State,
}

impl<'a, 'b: 'a, T: TrackedSocket + 'a> SocketTracker<'a, 'b, T> {
    pub(crate) fn new(dispatch_table: &'a mut DispatchTable,
           dirty_sockets: &'a mut RingBuffer<'b, SocketHandle>,
           handle: SocketHandle,
           socket: &'a mut T) -> Self {
        let state = TrackedSocket::new_state(socket);
        SocketTracker {
            handle,
            dispatch_table,
            socket,
            state,
            dirty_sockets,
        }
    }
}

impl<'a, 'b: 'a, T: TrackedSocket + 'a> Drop for SocketTracker<'a, 'b, T> {
    fn drop(&mut self) {
        TrackedSocket::on_drop(&self.state, self.dispatch_table, self.socket, self.handle);
        if !TrackedSocket::is_on_dirty_list(self.socket) &&
            TrackedSocket::is_dirty(self.socket) {
            match self.dirty_sockets.enqueue() {
                Ok(h) => {
                    *h = self.handle;
                    TrackedSocket::set_on_dirty_list(self.socket, true);
                }
                _ => {
                    unreachable!();
                }
            }
        }
    }
}

impl<'a, 'b: 'a, T: TrackedSocket + 'a> Deref for SocketTracker<'a, 'b, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.socket
    }
}

impl<'a, 'b: 'a, T: TrackedSocket + 'a> DerefMut for SocketTracker<'a, 'b, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.socket
    }
}
