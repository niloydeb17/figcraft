/// What can go wrong signing in or calling the API. Messages are for the person running the
/// command; none carries a token.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// The connection or the TLS handshake failed.
    #[error("network error: {0}")]
    Network(String),
    /// The server answered with an error status.
    #[error("the server answered {status}: {body}")]
    Status { status: u16, body: String },
    /// The sign-in did not complete.
    #[error("sign-in failed: {0}")]
    Auth(String),
    /// The sign-in was not finished in time.
    #[error("sign-in timed out: no browser callback arrived")]
    Timeout,
    /// A reply was not what the protocol describes.
    #[error("unexpected reply: {0}")]
    Reply(String),
    /// The credentials file could not be read or written.
    #[error("credentials file: {0}")]
    Store(String),
    /// There are no stored credentials, or they can no longer be refreshed.
    #[error("not signed in: run `figcraft-cli ai login`")]
    NotSignedIn,
    /// The system gave no randomness (never expected).
    #[error("no randomness: {0}")]
    Random(String),
}
