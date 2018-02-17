extern crate smoltcp;

use smoltcp::layers::link::loopback;
use smoltcp::layers::route::default_route;
use smoltcp::layers::{Emitter, IfaceDescr, Layer, Poller, SizeReq};
use smoltcp::phy::DeviceCapabilities;
use smoltcp::wire::{IpProtocol, Ipv4Address, Ipv4Packet, Ipv4Repr};

fn fill_in_ipv4(buf: &mut [u8], dev_caps: &DeviceCapabilities) {
    let ipv4_repr = Ipv4Repr {
        src_addr: Ipv4Address::new(0x7f, 0x00, 0x00, 0x02),
        dst_addr: Ipv4Address::new(0x7f, 0x00, 0x00, 0x01),
        protocol: IpProtocol::Icmp,
        payload_len: 0,
        hop_limit: 64,
    };
    let mut packet = Ipv4Packet::new(buf);
    ipv4_repr.emit(&mut packet, &dev_caps.checksum);
}

fn main() {
    println!("Layers test");
    let mut stack = default_route::Layer::new(loopback::Layer::new());
    let routing_data = smoltcp::layers::RoutingData::unknown_iface(smoltcp::wire::IpAddress::Ipv4(
        smoltcp::wire::Ipv4Address([127, 0, 0, 1]),
    ));
    {
        let (mut emitter, _) = stack.split();
        emitter
            .emit(&routing_data, &SizeReq::Exactly(40), &mut |buf, iface| {
                fill_in_ipv4(buf, iface.dev_caps());
                Ok(())
            })
            .expect("Can't emit");
    }
    println!("{:?}", stack);
    stack.sync();
    println!("{:?}", stack);
    {
        let (mut emitter, mut poller) = Layer::split(&mut stack);
        poller
            .poll(|_, packet| {
                if let smoltcp::layers::IpPacket::Ipv4(ipv4_packet) = packet {
                    let routing_data = smoltcp::layers::RoutingData::unknown_iface(ipv4_packet.src_addr());
                    emitter.emit(
                        &routing_data,
                        &SizeReq::Exactly(ipv4_packet.as_ref().len() as u16),
                        &mut |buf, _iface| {
                            let mut i = 0;
                            for b in ipv4_packet.as_ref().iter() {
                                buf[i] = *b;
                                i += 1;
                            }
                            Ok(())
                        },
                    )
                } else {
                    Ok(())
                }
            })
            .expect("Can't poll")
            .expect("Can't poll");
    }
    println!("{:?}", stack);
}
