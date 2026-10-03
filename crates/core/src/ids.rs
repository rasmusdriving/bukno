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
id_type!(
    /// A folder Bukno runs work in: a project's checkout or a projectless chat folder.
    WorkspaceId
);
id_type!(
    /// A project the user added to the sidebar.
    ProjectId
);
id_type!(
    /// One approval or question the engine is waiting on.
    DecisionId
);

macro_rules! id_text {
    ($($name:ident),*) => {$(
        impl $name {
            /// The identifier as 32 lowercase hex digits, for storage and wire IDs.
            pub fn hex(self) -> String {
                format!("{:032x}", self.0)
            }

            pub fn from_hex(text: &str) -> Option<Self> {
                u128::from_str_radix(text, 16).ok().map(Self)
            }
        }
    )*};
}

id_text!(TaskId, RunId, MessageId, ItemId, WorkspaceId, ProjectId, DecisionId);
