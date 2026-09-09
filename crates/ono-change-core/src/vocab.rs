//! The macro every closed vocabulary of v0.6 is written with.
//!
//! The specification fixes a dozen small enumerations — effect confidence, impact class,
//! protection level, consistency class, risk class, recovery objective, and so on — and says of
//! several of them that the list is closed. A closed list is only closed if nothing can widen it
//! silently, so each one is declared once here with its spelling, its documentation and its
//! `ALL`, and `xtask` compares `ALL` against the machine-readable registry in both directions.

/// Declares a closed vocabulary: a fieldless enum with a wire spelling, `ALL` and `from_name`.
macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        $name:ident {
            $( $variant:ident => $spelling:literal, $doc:literal; )*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $( #[doc = $doc] $variant, )*
        }

        impl $name {
            /// Every member, in the order the specification lists them.
            pub const ALL: &'static [$name] = &[ $( $name::$variant, )* ];

            /// The spelling this member carries in contracts, records and rendered output.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $( $name::$variant => $spelling, )* }
            }

            /// Reads a member back from its spelling, or `None` for a word not in the list.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $( $spelling => Some($name::$variant), )* _ => None }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

pub(crate) use vocabulary;
