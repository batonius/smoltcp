#[cfg(any(feature = "std", feature = "collections"))]
mod dispatch_table;
#[cfg(any(feature = "std", feature = "collections"))]
pub(crate) use self::dispatch_table::{DispatchTable};
#[cfg(not(any(feature = "std", feature = "collections")))]
mod dispatch_table_nostd;
#[cfg(not(any(feature = "std", feature = "collections")))]
pub(crate) use self::dispatch_table_nostd::{DispatchTable};
