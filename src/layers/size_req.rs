pub enum SizeReq {
    Exactly(u16),
    AtLeast(u16),
    Range(u16, u16),
    Max,
}

impl SizeReq {
    pub fn optimal_size(&self, mtu: usize) -> Option<usize> {
        match *self {
            SizeReq::Exactly(v) => {
                if v as usize <= mtu {
                    Some(v as usize)
                } else {
                    None
                }
            }
            SizeReq::AtLeast(v) => {
                if v as usize > mtu {
                    None
                } else {
                    Some(mtu)
                }
            }
            SizeReq::Range(from, to) => {
                if from as usize > mtu {
                    None
                } else {
                    Some(::std::cmp::min(mtu, to as usize))
                }
            }
            SizeReq::Max => Some(mtu),
        }
    }
}
