//! A tiny extension for the pervasive `Result<String, String>` "command output
//! or error message" shape. Many panes run an operation and show either the
//! output or the error verbatim in the same text area; `.text()` names that
//! collapse so the intent is explicit and lives in one place (e.g. if we ever
//! want to prefix errors, it's a single edit).
//!
//! Deliberately *not* a typed error enum: every call site in the app displays
//! the message as-is — none branch on the error kind — so an enum would be
//! stringified immediately everywhere, adding machinery with no consumer.

pub trait ResultText {
    /// Collapse an `Ok(output)` / `Err(message)` pair into the string to show.
    fn text(self) -> String;
}

impl ResultText for Result<String, String> {
    fn text(self) -> String {
        self.unwrap_or_else(|message| message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_returns_ok_or_err_verbatim() {
        assert_eq!(Ok::<_, String>("out".to_owned()).text(), "out");
        assert_eq!(Err::<String, _>("boom".to_owned()).text(), "boom");
    }
}
