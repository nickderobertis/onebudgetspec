//! Why an invocation is refused before anything runs.

use std::fmt;

use crate::report::exit;

/// A refusal: the invocation or a budgets file is invalid, and no command was run.
///
/// Every variant maps to exit status `2`. A measurement that fails is never an `Error`:
/// it is a result whose verdict is `error`, reported beside the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The invocation names something that is not there: a file that does not exist, a
    /// directory without `--recursive`, or an id no selected file registers.
    Invocation {
        /// What is wrong.
        message: String,
        /// What to do about it.
        next: String,
    },
    /// One or more budgets files are invalid. Each problem names the file and the key.
    Invalid {
        /// One line per problem, each starting with the file's path.
        problems: Vec<String>,
    },
}

impl Error {
    /// The exit status this refusal earns: always `2`.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        exit::INVALID
    }

    pub(crate) fn invocation(message: impl Into<String>, next: impl Into<String>) -> Self {
        Self::Invocation {
            message: message.into(),
            next: next.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invocation { message, next } => write!(f, "{message}\nnext: {next}"),
            Self::Invalid { problems } => {
                for problem in problems {
                    writeln!(f, "{problem}")?;
                }
                write!(
                    f,
                    "next: fix the {} above, then re-run; nothing was measured",
                    if problems.len() == 1 {
                        "problem"
                    } else {
                        "problems"
                    }
                )
            }
        }
    }
}

impl std::error::Error for Error {}
