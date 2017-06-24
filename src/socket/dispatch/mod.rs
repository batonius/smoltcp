#[cfg(any(feature = "std", feature = "collections"))]
mod dispatch_table;
#[cfg(any(feature = "std", feature = "collections"))]
pub use self::dispatch_table::{DispatchTable, Iter};
#[cfg(not(any(feature = "std", feature = "collections")))]
mod dispatch_table_nostd;
#[cfg(not(any(feature = "std", feature = "collections")))]
pub use self::dispatch_table_nostd::{DispatchTable, Iter};
