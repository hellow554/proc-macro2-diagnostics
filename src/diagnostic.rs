use proc_macro2::{Span, TokenStream};

use crate::SpanDiagExt;

/// Trait implemented by types that can be converted into a set of `Span`s.
pub trait MultiSpan {
    /// Converts `self` into a `Vec<Span>`.
    fn into_spans(self) -> Vec<Span>;
}

impl MultiSpan for Span {
    fn into_spans(self) -> Vec<Span> { vec![self] }
}

impl MultiSpan for Vec<Span> {
    fn into_spans(self) -> Vec<Span> { self }
}

impl<'a> MultiSpan for &'a [Span] {
    fn into_spans(self) -> Vec<Span> {
        self.to_vec()
    }
}

/// An enum representing a diagnostic level.
#[derive(Copy, Clone, Debug)]
pub enum Level {
    /// An error.
    Error,
    /// A warning.
    Warning,
    /// A note.
    Note,
    /// A help message.
    Help,
    #[doc(hidden)]
    __NonExhaustive
}

/// A structure representing a diagnostic message and associated children
/// messages.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    level: Level,
    message: String,
    spans: Vec<Span>,
    children: Vec<Diagnostic>
}

macro_rules! diagnostic_child_methods {
    ($spanned:ident, $regular:ident, $level:expr) => (
        /// Adds a new child diagnostic message to `self` with the level
        /// identified by this method's name with the given `spans` and
        /// `message`.
        pub fn $spanned<S, T>(mut self, spans: S, message: T) -> Diagnostic
            where S: MultiSpan, T: Into<String>
        {
            self.children.push(Diagnostic::spanned(spans, $level, message));
            self
        }

        /// Adds a new child diagnostic message to `self` with the level
        /// identified by this method's name with the given `message`.
        pub fn $regular<T: Into<String>>(mut self, message: T) -> Diagnostic {
            self.children.push(Diagnostic::new($level, message));
            self
        }
    )
}

impl Diagnostic {
    /// Creates a new diagnostic with the given `level` and `message`.
    pub fn new<T: Into<String>>(level: Level, message: T) -> Diagnostic {
        Diagnostic {
            level: level,
            message: message.into(),
            spans: vec![],
            children: vec![]
        }
    }

    /// Creates a new diagnostic with the given `level` and `message` pointing
    /// to the given set of `spans`.
    pub fn spanned<S, T>(spans: S, level: Level, message: T) -> Diagnostic
        where S: MultiSpan, T: Into<String>
    {
        Diagnostic {
            level: level,
            message: message.into(),
            spans: spans.into_spans(),
            children: vec![]
        }
    }

    diagnostic_child_methods!(span_error, error, Level::Error);
    diagnostic_child_methods!(span_warning, warning, Level::Warning);
    diagnostic_child_methods!(span_note, note, Level::Note);
    diagnostic_child_methods!(span_help, help, Level::Help);

    /// Return the children diagnostics of `self`.
    pub fn children(&self) -> impl Iterator<Item=&Diagnostic> {
        self.children.iter()
    }

    /// Return the `level` of `self`.
    pub fn level(&self) -> Level {
        self.level
    }

    fn stable_emit_as_tokens(self) -> TokenStream {
        let error: syn::parse::Error = self.into();
        error.to_compile_error()
    }

    /// Emit the diagnostic as tokens.
    #[cfg(not(nightly_diagnostics))]
    pub fn emit_as_tokens(self) -> TokenStream {
        self.stable_emit_as_tokens()
    }

    /// Emit the diagnostic as tokens.
    #[cfg(nightly_diagnostics)]
    pub fn emit_as_tokens(self) -> TokenStream {
        if !crate::nightly_works() {
            return self.stable_emit_as_tokens();
        }

        let nightly_diag: proc_macro::Diagnostic = self.into();
        nightly_diag.emit();
        TokenStream::new()
    }
}

const WARN_PREFIX: &str = "[warning] ";
const NOTE_PREFIX: &str = "[note] ";
const HELP_PREFIX: &str = "[help] ";

const JOINED_WARN_PREFIX: &str = "[<- warning] ";
const JOINED_NOTE_PREFIX: &str = "[<- note] ";
const JOINED_HELP_PREFIX: &str = "[<- help] ";
const JOINED_ERROR_PREFIX: &str = "[<- error] ";

impl Into<syn::parse::Error> for Diagnostic {
    fn into(self) -> syn::parse::Error {
        fn diag_to_msg(diag: &Diagnostic) -> String {
            let spans = &diag.spans;
            let msg_prefix = match (spans.is_empty(), diag.level) {
                (true, Level::Warning) => JOINED_WARN_PREFIX,
                (false, Level::Warning) => WARN_PREFIX,
                (true, Level::Note) => JOINED_NOTE_PREFIX,
                (false, Level::Note) => NOTE_PREFIX,
                (true, Level::Help) => JOINED_HELP_PREFIX,
                (false, Level::Help) => HELP_PREFIX,
                (true, Level::Error) => JOINED_ERROR_PREFIX,
                _ => ""
            };

            format!("{}{}", msg_prefix, diag.message)
        }

        fn diag_to_span(diag: &Diagnostic) -> Span {
            let spans = &diag.spans;
            if spans.is_empty() {
                Span::call_site()
            } else {
                spans[0]
            }
        }

        let span = diag_to_span(&self);
        let mut error = syn::parse::Error::new(span, diag_to_msg(&self));
        for child in self.children {
            let child_error = if child.spans.is_empty() {
                syn::parse::Error::new(span, diag_to_msg(&child))
            } else {
                syn::parse::Error::new(diag_to_span(&child), diag_to_msg(&child))
            };

            error.combine(child_error);
        }

        error
    }
}

impl From<syn::parse::Error> for Diagnostic {
    fn from(error: syn::parse::Error) -> Diagnostic {
        let mut diag = error.span().error(error.to_string());
        for e in error.into_iter().skip(1) {
            let message = e.to_string();
            if message.starts_with(WARN_PREFIX) {
                let message = &message[WARN_PREFIX.len()..];
                diag = diag.span_warning(e.span(), message.to_string());
            } else if message.starts_with(JOINED_WARN_PREFIX) {
                let message = &message[JOINED_WARN_PREFIX.len()..];
                diag = diag.warning(message.to_string());
            } else if message.starts_with(NOTE_PREFIX) {
                let message = &message[NOTE_PREFIX.len()..];
                diag = diag.span_note(e.span(), message.to_string());
            } else if message.starts_with(JOINED_NOTE_PREFIX) {
                let message = &message[JOINED_NOTE_PREFIX.len()..];
                diag = diag.note(message.to_string());
            } else if message.starts_with(HELP_PREFIX) {
                let message = &message[HELP_PREFIX.len()..];
                diag = diag.span_help(e.span(), message.to_string());
            } else if message.starts_with(JOINED_HELP_PREFIX) {
                let message = &message[JOINED_HELP_PREFIX.len()..];
                diag = diag.help(message.to_string());
            } else if message.starts_with(JOINED_ERROR_PREFIX) {
                let message = &message[JOINED_ERROR_PREFIX.len()..];
                diag = diag.error(message.to_string());
            } else {
                diag = diag.span_error(e.span(), e.to_string());
            }
        }

        diag
    }
}

#[cfg(nightly_diagnostics)]
impl Into<proc_macro::Diagnostic> for Diagnostic {
    fn into(self) -> proc_macro::Diagnostic {
        fn spans_to_proc_macro_spans(spans: Vec<Span>) -> Vec<proc_macro::Span> {
            spans.into_iter()
                .map(|s| s.unstable())
                .collect::<Vec<proc_macro::Span>>()
        }

        let spans = spans_to_proc_macro_spans(self.spans);

        let level = match self.level {
            Level::Error => proc_macro::Level::Error,
            Level::Warning => proc_macro::Level::Warning,
            Level::Note => proc_macro::Level::Note,
            Level::Help => proc_macro::Level::Help,
            Level::__NonExhaustive => unreachable!("{:?}", self.level),
        };

        let mut diag = proc_macro::Diagnostic::spanned(spans, level, self.message);
        for child in self.children {
            // FIXME: proc_macro::Diagnostic needs a `push` method.
            let spans = spans_to_proc_macro_spans(child.spans);
            diag = match child.level {
                Level::Error => diag.span_error(spans, child.message),
                Level::Warning => diag.span_warning(spans, child.message),
                Level::Note => diag.span_note(spans, child.message),
                Level::Help => diag.span_help(spans, child.message),
                Level::__NonExhaustive => unreachable!("{:?}", child.level),
            };
        }

        diag
    }
}