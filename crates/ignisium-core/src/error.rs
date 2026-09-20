use std::fmt;

/// Errors produced by fallible text operations.
///
/// Invalid input is reported as a value instead of panicking, so a host
/// application can decide whether to clamp the offset, show a message or abort
/// the edit. Panicking would take the whole editor down for what is usually a
/// recoverable programming or input mistake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextError {
    /// The offset is past the end of the text.
    OffsetOutOfRange {
        offset: usize,
        /// Text length in UTF-8 bytes.
        len: usize,
    },

    /// The offset points into the middle of a UTF-8 multi-byte character.
    NotCharBoundary { offset: usize },

    /// The range is unusable: `start` is after `end`, or an endpoint is invalid.
    InvalidRange { start: usize, end: usize },
}

/// Shorthand for results of text operations.
pub type TextResult<T> = Result<T, TextError>;

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextError::OffsetOutOfRange { offset, len } => {
                write!(
                    f,
                    "offset {offset} is out of range: text length is {len} bytes"
                )
            }
            TextError::NotCharBoundary { offset } => {
                write!(f, "offset {offset} is not a UTF-8 character boundary")
            }
            TextError::InvalidRange { start, end } => {
                write!(f, "invalid text range [{start}, {end})")
            }
        }
    }
}

impl std::error::Error for TextError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_message_describes_the_problem() {
        let error = TextError::NotCharBoundary { offset: 1 };

        assert_eq!(
            error.to_string(),
            "offset 1 is not a UTF-8 character boundary"
        );
    }

    #[test]
    fn error_can_be_used_as_std_error() {
        // `std::error::Error` makes TextError composable with `?` in code that
        // returns `Box<dyn Error>`, and usable by error-reporting libraries.
        let error = TextError::OffsetOutOfRange { offset: 9, len: 6 };
        let boxed: Box<dyn std::error::Error> = Box::new(error);

        assert!(boxed.to_string().contains("out of range"));
    }
}
