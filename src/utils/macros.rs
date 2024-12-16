/// Defines a `FromStr` implementation for an enum with variants
/// that can be parsed from a string
///
/// Variants are prioritized in the order they are defined
/// in the macro, so any variant with an underlying type of `String`
/// will block any other variants that come after it
///
/// # Example
/// ```
/// #[derive(Debug, PartialEq)]
/// enum MyEnum {
///     A(u64),
///     B(String),
/// }
///
/// parse_variants!(MyEnum {
///     A as u64,
///     B as String,
/// });
///
/// assert_eq!(
///     "123".parse::<MyEnum>().unwrap(),
///     MyEnum::A(123)
/// );
///
/// assert_eq!(
///     "hello".parse::<MyEnum>().unwrap(),
///     MyEnum::B("hello".to_string())
/// );
/// ```
macro_rules! parse_variants {
    ($enum_name:ident { $( $variant:ident as $ty:ty ),* $(,)? }) => {
        impl std::str::FromStr for $enum_name {
            type Err = color_eyre::eyre::Report;

            #[allow(irrefutable_let_patterns)]
            fn from_str(s: &str) -> color_eyre::eyre::Result<Self, Self::Err> {
                $( if let Ok(v) = s.parse::<$ty>() {
                    return Ok(Self::$variant(v.into()));
                } else )* {
                    return Err(color_eyre::eyre::eyre!("Failed to parse input string: {s}"));
                }
            }
        }
    };
}

pub(crate) use parse_variants;

#[cfg(test)]
mod tests {
    #[test]
    fn parse_variants() {
        #[derive(Debug, PartialEq)]
        enum MyEnum {
            A(u8),
            B(u64),
            C(String),
            D(u64),
        }

        parse_variants!(MyEnum {
            A as u8,
            B as u16,
            C as String,
            D as u64,
        });

        assert_eq!("123".parse::<MyEnum>().unwrap(), MyEnum::A(123));
        assert_eq!("12345".parse::<MyEnum>().unwrap(), MyEnum::B(12345)); // larger than u8
        assert_eq!(
            "hello".parse::<MyEnum>().unwrap(),
            MyEnum::C("hello".to_string())
        );
        // D is never parsed because C is a String
        assert_eq!(
            "1234567".parse::<MyEnum>().unwrap(),
            MyEnum::C("1234567".to_string())
        )
    }
}
