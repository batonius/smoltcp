//! Communication between endpoints.
//!
//! The `socket` module deals with *network endpoints* and *buffering*.
//! It provides interfaces for accessing buffers of data, and protocol state machines
//! for filling and emptying these buffers.
//!
//! The programming interface implemented here differs greatly from the common Berkeley socket
//! interface. Specifically, in the Berkeley interface the buffering is implicit:
//! the operating system decides on the good size for a buffer and manages it.
//! The interface implemented by this module uses explicit buffering: you decide on the good
//! size for a buffer, allocate it, and let the networking stack use it.

use Error;
use phy::DeviceLimits;
use wire::IpRepr;

mod raw;
mod udp;
mod tcp;
mod dispatch;
mod set;

pub use self::raw::PacketBuffer as RawPacketBuffer;
pub use self::raw::SocketBuffer as RawSocketBuffer;
pub use self::raw::RawSocket;

pub use self::udp::PacketBuffer as UdpPacketBuffer;
pub use self::udp::SocketBuffer as UdpSocketBuffer;
pub use self::udp::UdpSocket;

pub use self::tcp::SocketBuffer as TcpSocketBuffer;
pub use self::tcp::State as TcpState;
pub use self::tcp::TcpSocket;

pub use self::set::{Set as SocketSet, Item as SocketSetItem, Handle as SocketHandle};
pub use self::set::{Iter as SocketSetIter, IterMut as SocketSetIterMut};

pub use self::dispatch::{DispatchTable as SocketDispatchTable, Iter as SocketDispatchIterMut};

/// A network socket.
///
/// This enumeration abstracts the various types of sockets based on the IP protocol.
/// To downcast a `Socket` value down to a concrete socket, use
/// the [AsSocket](trait.AsSocket.html) trait, and call e.g. `socket.as_socket::<UdpSocket<_>>()`.
///
/// The `process` and `dispatch` functions are fundamentally asymmetric and thus differ in
/// their use of the [trait PacketRepr](trait.PacketRepr.html). When `process` is called,
/// the packet length is already known and no allocation is required; on the other hand,
/// `process` would have to downcast a `&PacketRepr` to e.g. an `&UdpRepr` through `Any`,
/// which is rather inelegant. Conversely, when `dispatch` is called, the packet length is
/// not yet known and the packet storage has to be allocated; but the `&PacketRepr` is sufficient
/// since the lower layers treat the packet as an opaque octet sequence.
#[derive(Debug)]
pub enum Socket<'a, 'b: 'a> {
    Raw(RawSocket<'a, 'b>),
    Udp(UdpSocket<'a, 'b>),
    Tcp(TcpSocket<'a>),
    #[doc(hidden)]
    __Nonexhaustive
}

macro_rules! dispatch_socket {
    ($self_:expr, |$socket:ident [$( $mut_:tt )*]| $code:expr) => ({
        match $self_ {
            &$( $mut_ )* Socket::Raw(ref $( $mut_ )* $socket) => $code,
            &$( $mut_ )* Socket::Udp(ref $( $mut_ )* $socket) => $code,
            &$( $mut_ )* Socket::Tcp(ref $( $mut_ )* $socket) => $code,
            &$( $mut_ )* Socket::__Nonexhaustive => unreachable!()
        }
    })
}

impl<'a, 'b> Socket<'a, 'b> {
    /// Return the debug identifier.
    pub fn debug_id(&self) -> usize {
        dispatch_socket!(self, |socket []| socket.debug_id())
    }

    /// Set the debug identifier.
    ///
    /// The debug identifier is a number printed in socket trace messages.
    /// It could as well be used by the user code.
    pub fn set_debug_id(&mut self, id: usize) {
        dispatch_socket!(self, |socket [mut]| socket.set_debug_id(id))
    }

    pub(crate) fn dispatch<F, R>(&mut self, timestamp: u64, limits: &DeviceLimits,
                                 emit: &mut F) -> Result<R, Error>
            where F: FnMut(&IpRepr, &IpPayload) -> Result<R, Error> {
        dispatch_socket!(self, |socket [mut]| socket.dispatch(timestamp, limits, emit))
    }
}

/// An IP-encapsulated packet representation.
///
/// This trait abstracts the various types of packets layered under the IP protocol,
/// and serves as an accessory to [trait Socket](trait.Socket.html).
pub trait IpPayload {
    /// Return the length of the buffer required to serialize this high-level representation.
    fn buffer_len(&self) -> usize;

    /// Emit this high-level representation into a sequence of octets.
    fn emit(&self, ip_repr: &IpRepr, payload: &mut [u8]);
}

/// A conversion trait for network sockets.
///
/// This trait is used to concisely downcast [Socket](trait.Socket.html) values to their
/// concrete types.
pub trait AsSocket<T> {
    fn as_socket(&mut self) -> &mut T;
    fn try_as_socket(&mut self) -> Option<&mut T>;
}

macro_rules! as_socket {
    ($socket:ty, $variant:ident) => {
        impl<'a, 'b> AsSocket<$socket> for Socket<'a, 'b> {
            fn as_socket(&mut self) -> &mut $socket {
                match self {
                    &mut Socket::$variant(ref mut socket) => socket,
                    _ => panic!(concat!(".as_socket::<",
                                        stringify!($socket),
                                        "> called on wrong socket type"))
                }
            }

            fn try_as_socket(&mut self) -> Option<&mut $socket> {
                match self {
                    &mut Socket::$variant(ref mut socket) => Some(socket),
                    _ => None,
                }
            }
        }
    }
}

as_socket!(RawSocket<'a, 'b>, Raw);
as_socket!(UdpSocket<'a, 'b>, Udp);
as_socket!(TcpSocket<'a>, Tcp);
