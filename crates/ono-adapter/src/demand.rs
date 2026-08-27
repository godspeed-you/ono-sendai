//! What a consumer demands of a child process's stdout (spec v0.3 §1.4, §1.5).

use std::fmt;

/// The kind of output a stage's stdout must carry, decided by what is attached to it.
///
/// The demand is computed backwards from the consumer while the pipeline is planned, never
/// guessed from what the program could produce: it is what lets `ss -tunap | grep ':443'` stay
/// the pipeline it has always been while `ss -tunap | where state == established` may be
/// adapted (spec v0.3 §1.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputDemand {
    /// Bytes, untouched: the consumer is a process, a file or an inherited stream.
    RawBytes,
    /// Text: the consumer is declared over strings alone.
    Text,
    /// Values, optionally of one schema: the consumer is a native command over objects.
    Structured {
        /// The schema the consumer declares, when it declares exactly one.
        schema: Option<String>,
    },
    /// Whatever renders best: the consumer is the terminal.
    Interactive,
    /// Nothing at all: stdout goes to `/dev/null`.
    Discard,
}

/// What is attached to a stage's stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consumer<'a> {
    /// The next stage is a child process, joined by a real pipe.
    Process,
    /// The next stage is a native command whose declared input type is `input`, spelled the
    /// way the registry spells it (`stream<ono.process/1>`, `string | bytes`, …).
    Native {
        /// The consumer's declared input type.
        input: &'a str,
    },
    /// stdout is redirected to a path.
    File {
        /// The path as written.
        path: &'a str,
    },
    /// stdout is duplicated onto another descriptor, as in `>&2`.
    Descriptor,
    /// stdout is the terminal.
    Terminal,
    /// stdout is a pipe or file the shell inherited — a script, `ono -c` under another program.
    Stream,
}

/// Where the shell's own stdout goes, for the stage that has no consumer inside the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stdout {
    /// A terminal: the renderer is the consumer.
    Terminal,
    /// Anything else: a pipe or a file some other program reads.
    Stream,
}

impl OutputDemand {
    /// The demand `consumer` places on the stdout it is attached to.
    #[must_use]
    pub fn for_consumer(consumer: Consumer<'_>) -> Self {
        match consumer {
            Consumer::Process | Consumer::Descriptor | Consumer::Stream => Self::RawBytes,
            Consumer::Native { input } => Self::for_input(input),
            Consumer::File { path } => {
                if path == "/dev/null" {
                    Self::Discard
                } else {
                    Self::RawBytes
                }
            }
            Consumer::Terminal => Self::Interactive,
        }
    }

    /// The demand of a native consumer declared over `input`.
    ///
    /// A consumer that admits bytes keeps them, because it is the user decoding them; one that
    /// admits only text wants text; everything else is a command over values, constrained to a
    /// schema when the declaration names exactly one.
    fn for_input(input: &str) -> Self {
        let alternatives: Vec<&str> = input
            .split('|')
            .map(str::trim)
            .filter(|part| *part != "null")
            .collect();
        let admits = |predicate: fn(&str) -> bool| alternatives.iter().copied().any(predicate);
        if admits(|part| part == "bytes" || part.starts_with("bytes")) {
            return Self::RawBytes;
        }
        if admits(|part| part == "string" || part.starts_with("string")) {
            return Self::Text;
        }
        let schemas: Vec<&str> = alternatives
            .iter()
            .map(|part| {
                part.strip_prefix("stream<")
                    .and_then(|inner| inner.strip_suffix('>'))
                    .unwrap_or(part)
            })
            .filter(|element| element.contains('/'))
            .collect();
        Self::Structured {
            schema: match schemas.as_slice() {
                [only] => Some((*only).to_owned()),
                _ => None,
            },
        }
    }
}

impl fmt::Display for OutputDemand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RawBytes => f.write_str("bytes"),
            Self::Text => f.write_str("text"),
            Self::Structured { schema: None } => f.write_str("structured"),
            Self::Structured {
                schema: Some(schema),
            } => write!(f, "structured<{schema}>"),
            Self::Interactive => f.write_str("interactive"),
            Self::Discard => f.write_str("discard"),
        }
    }
}
