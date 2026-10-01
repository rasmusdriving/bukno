//! Typed identifiers. The runtime creates them (as UUIDs) and passes them in;
//! core only compares and stores them.

use std::fmt;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u128);

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({:032x})", stringify!($name), self.0)
            }
        }
    };
}

id_type!(
    /// The durable conversation identity the user sees as a chat.
    TaskId
);
id_type!(
    /// One attempt to perform submitted work, normally one provider turn.
    RunId
);
id_type!(
    /// One user message and its delivery record.
    MessageId
);
id_type!(
    /// One transcript item: a user message or an agent reply.
    ItemId
);
